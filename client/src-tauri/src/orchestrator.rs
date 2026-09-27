//! ホットキー・音声・WebSocket・状態機械・挿入をつなぐ実行ループ (design.md §5.2〜§5.5)。
//!
//! Tauri のグローバルショートカットハンドラは同期関数なので、ここでは
//! `mpsc` チャンネル越しにイベントを受け取り、非同期タスクとして駆動する。
//! 個々のロジック (状態機械・挿入判定・チャンク分割等) は各モジュールで単体テスト済み。
//! このファイル自体は Tauri の実行ランタイムと実デバイスに依存するため結合テストの対象外だが、
//! マイクのルーティング判定 ([`route_audio_chunk`]) だけは cpal/AppHandle 抜きで単体テストする。

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::accessibility;
use crate::audio::{self, PreRollBuffer};
use crate::insert::{self, FrontApp};
use crate::overlay;
use crate::protocol::{ClientMessage, Flag, Mode, ServerMessage, Timings};
use crate::settings::{self, History, HistoryItem, InsertMethod, Settings};
use crate::state::{Outcome, PressOutcome, StateMachine};
use crate::status::StatusStore;
use crate::ws::{OutgoingFrame, PendingQueue};

/// `voice://result` (final 受信ごと) のイベント名。
const RESULT_EVENT: &str = "voice://result";
/// `voice://level` (録音中 50ms ごと、波形表示用) のイベント名。
const LEVEL_EVENT: &str = "voice://level";
/// `voice://insert_failed` のイベント名。
const INSERT_FAILED_EVENT: &str = "voice://insert_failed";

#[derive(Debug, Clone, Copy, Serialize)]
struct LevelPayload {
    rms: f32,
}

#[derive(Debug, Clone, Serialize)]
struct InsertFailedPayload {
    id: String,
    reason: &'static str,
    message: String,
}

/// `final` 待ちタイムアウト = 5秒 + 発話長の10% (design.md §5.4)。
pub fn final_wait_timeout(utterance: Duration) -> Duration {
    Duration::from_secs(5) + utterance.mul_f64(0.10)
}

/// 失敗しても致命的ではない (呼び出し元の処理は続行する) が、黒握りつぶしはしない操作の共通ログ。
/// (code-quality: エラーを握りつぶさない — 最低限ログを出す)
fn log_on_err<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) {
    if let Err(e) = result {
        tracing::warn!("{context}: {e}");
    }
}

#[derive(Debug)]
pub enum OrchestratorEvent {
    HotkeyPressed,
    HotkeyReleased,
    CancelRequested,
    AudioChunk(Uuid, Vec<u8>),
    ServerMessage(ServerMessage),
    /// `final` 待ちタイムアウト (§5.4)。まだ `WaitingFinal` のセッションだけを諦める
    /// ([`crate::state::StateMachine::on_timeout`] が判定する)。
    TimedOut(Uuid),
}

/// 押下時に記録しておく、この発話に関する付帯情報。
struct RecordingContext {
    front_app: FrontApp,
    started_at: std::time::Instant,
}

/// マイクからの PCM を「誰に配送するか」を持つ共有状態。cpal のコールバックスレッド (同期) と
/// Orchestrator (非同期) の両方から `std::sync::Mutex` 経由で読み書きする。
struct AudioRoute {
    preroll: PreRollBuffer,
    current_session: Option<Uuid>,
}

/// 現在開いているマイクストリームの持ち方。
enum MicHandle {
    /// マイクを開いていない (押している間だけ録音、かつ非録音中)。
    None,
    /// 「マイクを常時開いておく」設定 (design.md §5.4)。アプリの生存期間ずっと開けたままにし、
    /// 録音していない間もプリロールへ積み続ける。実際にどのセッションへ配送するかは `route` で切り替える。
    AlwaysOpen {
        _stream: cpal::Stream,
        route: Arc<StdMutex<AudioRoute>>,
    },
    /// 既定動作: 押している間だけ開く。解放/Esc で drop してマイクを閉じる。
    PerUtterance { _stream: cpal::Stream },
}

/// マイクコールバックから届いた1チャンクを、プリロールへ積みつつ現在のセッションへ配送するかを
/// 決める。cpal/Mutex の実際の I/O から分離した純粋ロジックとしてテストする
/// (レビュー指摘2: マイク配線自体はここでは検証できないので、この判定だけを切り出す)。
fn route_audio_chunk(
    route: &StdMutex<AudioRoute>,
    events_tx: &mpsc::UnboundedSender<OrchestratorEvent>,
    bytes: Vec<u8>,
) {
    let current_session = {
        let mut r = route.lock().unwrap_or_else(|p| p.into_inner());
        r.preroll.push(&bytes);
        r.current_session
    };
    if let Some(id) = current_session {
        log_on_err(
            events_tx.send(OrchestratorEvent::AudioChunk(id, bytes)),
            "PCM チャンクの送信に失敗",
        );
    }
}

/// 押下時: プリロールの現在のスナップショットを取り出しつつ、以後の配送先をこのセッションに切り替える。
/// 戻り値は「先頭に送るべきプリロール分」(空なら送る必要なし)。
fn snapshot_and_start_routing(route: &StdMutex<AudioRoute>, id: Uuid) -> Vec<u8> {
    let mut r = route.lock().unwrap_or_else(|p| p.into_inner());
    let snapshot = r.preroll.snapshot();
    r.current_session = Some(id);
    snapshot
}

/// 解放/Esc/常時オープンでの録音終了時: 以後の配送を止める (プリロールへの蓄積自体は続ける)。
fn stop_routing(route: &StdMutex<AudioRoute>) {
    let mut r = route.lock().unwrap_or_else(|p| p.into_inner());
    r.current_session = None;
}

pub struct Orchestrator {
    app: AppHandle,
    state: StateMachine,
    settings: Settings,
    outgoing: PendingQueue,
    events_tx: mpsc::UnboundedSender<OrchestratorEvent>,
    recordings: HashMap<Uuid, RecordingContext>,
    mic: MicHandle,
    status: Arc<StatusStore>,
    history: Arc<AsyncMutex<History>>,
}

impl Orchestrator {
    pub fn new(
        app: AppHandle,
        settings: Settings,
        outgoing: PendingQueue,
        events_tx: mpsc::UnboundedSender<OrchestratorEvent>,
        status: Arc<StatusStore>,
        history: Arc<AsyncMutex<History>>,
    ) -> Self {
        let mic = if settings.always_open_mic {
            Self::open_always_on_mic(&settings, &events_tx)
        } else {
            MicHandle::None
        };
        Self {
            app,
            state: StateMachine::new(),
            settings,
            outgoing,
            events_tx,
            recordings: HashMap::new(),
            mic,
            status,
            history,
        }
    }

    /// UI 契約の `phase` を現在の状態機械に同期させ、変わっていれば `voice://status` を発行する
    /// (発行自体の重複抑制は [`StatusStore`] 側が行う)。
    fn sync_phase(&self) {
        self.status.set_phase(&self.app, self.state.ui_phase().into());
    }

    /// 「マイクを常時開いておく」設定用に、アプリ起動時 (または設定変更時) にマイクを開く。
    /// 失敗してもアプリは起動を続ける (次に押下されたときに `PerUtterance` 相当を試みるべきだが、
    /// 現状は「常時オープン設定なのに開けなかった」場合は録音自体ができない状態として警告する)。
    fn open_always_on_mic(
        settings: &Settings,
        events_tx: &mpsc::UnboundedSender<OrchestratorEvent>,
    ) -> MicHandle {
        let route = Arc::new(StdMutex::new(AudioRoute {
            preroll: PreRollBuffer::new(),
            current_session: None,
        }));
        let route_for_callback = route.clone();
        let events_tx = events_tx.clone();
        match audio::open_input_stream(settings.mic_device.as_deref(), move |bytes| {
            route_audio_chunk(&route_for_callback, &events_tx, bytes);
        }) {
            Ok(stream) => MicHandle::AlwaysOpen { _stream: stream, route },
            Err(e) => {
                tracing::error!("マイクの常時オープンに失敗 (常時オープン設定): {e}");
                MicHandle::None
            }
        }
    }

    /// イベントループ本体。`mpsc::UnboundedReceiver` が閉じたら終了する。
    pub async fn run(mut self, mut events: mpsc::UnboundedReceiver<OrchestratorEvent>) {
        while let Some(event) = events.recv().await {
            self.handle(event).await;
        }
    }

    async fn handle(&mut self, event: OrchestratorEvent) {
        match event {
            OrchestratorEvent::HotkeyPressed => self.on_press(),
            OrchestratorEvent::HotkeyReleased => self.on_release(),
            OrchestratorEvent::CancelRequested => self.on_cancel(),
            OrchestratorEvent::AudioChunk(_id, bytes) => {
                // 現行プロトコルは接続内で単一セッションのみ想定 (README に明記)。
                self.emit_level_for_chunk(&bytes);
                // `PendingQueue::push` は同期・infallible なので `log_on_err` は不要。
                self.outgoing.push(OutgoingFrame::Pcm(bytes));
            }
            OrchestratorEvent::ServerMessage(msg) => self.on_server_message(msg).await,
            OrchestratorEvent::TimedOut(id) => self.on_timed_out(id).await,
        }
        // どの分岐でも状態機械が変わりうるので、最後に一括で UI 契約の phase を同期する
        // (AudioChunk のような無変化のケースは StatusStore 側が無音で無視する)。
        self.sync_phase();
    }

    /// UI 契約 `voice://level` (録音中 50ms ごと、波形表示用)。既存の 100ms PCM チャンクを
    /// 前後半 (各約50ms) に分けて RMS を計算する — 新たに生の音声を別経路で取り出すのではなく
    /// 「PCM から RMS を計算する」契約通り、既にルーティング済みのチャンクを再利用する。
    fn emit_level_for_chunk(&self, bytes: &[u8]) {
        let samples = audio::decode_pcm16le(bytes);
        if samples.is_empty() {
            return;
        }
        let mid = samples.len() / 2;
        for half in [&samples[..mid], &samples[mid..]] {
            if half.is_empty() {
                continue;
            }
            let payload = LevelPayload { rms: audio::rms(half) };
            if let Err(e) = self.app.emit(LEVEL_EVENT, payload) {
                tracing::warn!("{LEVEL_EVENT} イベントの発行に失敗: {e}");
            }
        }
    }

    fn on_press(&mut self) {
        let front_app = current_front_app_or_unknown();
        match self.state.on_hotkey_press(self.settings.output_mode) {
            PressOutcome::Started(id) => {
                self.recordings.insert(
                    id,
                    RecordingContext {
                        front_app,
                        started_at: std::time::Instant::now(),
                    },
                );
                self.outgoing.push(OutgoingFrame::Control(ClientMessage::start(
                    id,
                    self.settings.output_mode,
                )));
                self.start_mic_for_session(id);
                log_on_err(overlay::ensure_overlay_window(&self.app), "オーバーレイウィンドウの作成に失敗");
                log_on_err(
                    overlay::show_status(&self.app, overlay::OverlayStatus::Recording),
                    "オーバーレイ状態の更新に失敗",
                );
            }
            PressOutcome::IgnoredRepeat => {}
        }
    }

    /// レビュー指摘2: 実際にマイクを開き、コールバックから `AudioChunk` イベントを送るライフサイクル。
    /// 「常時オープン」設定なら既に開いているストリームの配送先を切り替えるだけ、
    /// そうでなければここで新規に `open_input_stream` する。
    fn start_mic_for_session(&mut self, id: Uuid) {
        match &self.mic {
            MicHandle::AlwaysOpen { route, .. } => {
                let preroll = snapshot_and_start_routing(route, id);
                if !preroll.is_empty() {
                    // 押下前の直近300ms分を、実際のマイク入力より先に送る (design.md §5.4)。
                    self.outgoing.push(OutgoingFrame::Pcm(preroll));
                }
            }
            MicHandle::None | MicHandle::PerUtterance { .. } => {
                let events_tx = self.events_tx.clone();
                match audio::open_input_stream(self.settings.mic_device.as_deref(), move |bytes| {
                    log_on_err(
                        events_tx.send(OrchestratorEvent::AudioChunk(id, bytes)),
                        "PCM チャンクの送信に失敗",
                    );
                }) {
                    Ok(stream) => self.mic = MicHandle::PerUtterance { _stream: stream },
                    Err(e) => tracing::error!("マイクを開けなかった: {e}"),
                }
            }
        }
    }

    /// 録音終了 (解放/Esc) 時にマイクを止める。常時オープンならストリームは開けたままにし、
    /// 配送先だけ外す。
    fn stop_mic(&mut self) {
        match &self.mic {
            MicHandle::AlwaysOpen { route, .. } => stop_routing(route),
            MicHandle::PerUtterance { .. } => self.mic = MicHandle::None,
            MicHandle::None => {}
        }
    }

    fn on_release(&mut self) {
        if let Some(id) = self.state.on_hotkey_release() {
            self.outgoing.push(OutgoingFrame::Control(ClientMessage::end(id)));
            self.stop_mic();
            log_on_err(
                overlay::show_status(&self.app, overlay::OverlayStatus::Processing),
                "オーバーレイ状態の更新に失敗",
            );

            let utterance_len = self
                .recordings
                .get(&id)
                .map(|c| c.started_at.elapsed())
                .unwrap_or(Duration::ZERO);
            let timeout = final_wait_timeout(utterance_len);
            let events_tx = self.events_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(timeout).await;
                // 送信失敗はアプリ終了によるチャンネルクローズのみが原因 (§7 の想定どおり)。
                log_on_err(events_tx.send(OrchestratorEvent::TimedOut(id)), "タイムアウト通知の送信に失敗");
            });
        }
    }

    fn on_cancel(&mut self) {
        if let Some(id) = self.state.on_cancel() {
            self.outgoing.push(OutgoingFrame::Control(ClientMessage::cancel(id)));
            self.stop_mic();
            self.recordings.remove(&id);
            log_on_err(overlay::hide(&self.app), "オーバーレイの非表示に失敗");
        }
    }

    async fn on_server_message(&mut self, msg: ServerMessage) {
        match msg {
            ServerMessage::Final {
                session_id,
                raw_text,
                text,
                mode,
                flags,
                timings,
            } => {
                self.record_history_if_non_empty(session_id, &raw_text, &text, mode, &flags, timings)
                    .await;
                self.finish_session(session_id, Some(text)).await;
            }
            ServerMessage::Error { session_id, message, .. } => {
                tracing::warn!("サーバーからエラー: {message}");
                self.status.set_last_error(&self.app, Some(message));
                if let Some(id) = session_id {
                    self.finish_session(id, None).await;
                }
            }
            ServerMessage::Ready { .. } | ServerMessage::Partial { .. } => {
                // partial は表示専用 (design.md §3.2)。オーバーレイのテキスト更新は将来拡張。
            }
        }
    }

    /// UI 契約 `voice://result` (final 受信ごと)。空発話 (挿入しない) は履歴に残さない。
    /// 挿入が成功したかどうか (`inserted`) は後で判明するため、ここでは `false` で記録し、
    /// [`perform_insert`] が成功したときに [`History::mark_inserted`] で更新する。
    async fn record_history_if_non_empty(
        &self,
        session_id: Uuid,
        raw_text: &str,
        text: &str,
        mode: Mode,
        flags: &[Flag],
        timings: Timings,
    ) {
        if text.is_empty() {
            return;
        }
        let item = HistoryItem {
            id: session_id.to_string(),
            at: settings::to_iso8601_utc(std::time::SystemTime::now()),
            mode,
            text: text.to_string(),
            raw_text: raw_text.to_string(),
            flags: flags.to_vec(),
            timings,
            inserted: false,
        };
        self.history.lock().await.push(item.clone());
        if let Err(e) = self.app.emit(RESULT_EVENT, item) {
            tracing::warn!("{RESULT_EVENT} イベントの発行に失敗: {e}");
        }
    }

    /// `final` 待ちタイムアウト。レビュー指摘3: すでに final を受信して `ReadyToInsert`/
    /// `Inserting` になっているセッションを誤って消さないよう、判定は
    /// `StateMachine::on_timeout` に委譲する (`WaitingFinal` のときだけ諦める)。
    async fn on_timed_out(&mut self, id: Uuid) {
        let outcome = self.state.on_timeout(id);
        self.forget_recording_if_session_gone(id);
        if self.state.is_idle() {
            log_on_err(overlay::hide(&self.app), "オーバーレイの非表示に失敗");
        }
        if let Outcome::Insert { id, text } = outcome {
            self.perform_insert(id, text).await;
        }
    }

    async fn finish_session(&mut self, id: Uuid, text: Option<String>) {
        let outcome = match text {
            Some(t) => self.state.on_final_received(id, t),
            None => self.state.on_error_or_timeout(id),
        };
        self.forget_recording_if_session_gone(id);
        if self.state.is_idle() {
            log_on_err(overlay::hide(&self.app), "オーバーレイの非表示に失敗");
        }
        if let Outcome::Insert { id, text } = outcome {
            self.perform_insert(id, text).await;
        }
    }

    /// セッションが (final 空・エラー・タイムアウトのいずれかで) 完全に消えたなら、
    /// `recordings` の付帯情報も一緒に片付ける。挿入待ち (`ReadyToInsert`/`Inserting`) の
    /// 間は残しておく (`perform_insert` が使うため)。
    fn forget_recording_if_session_gone(&mut self, id: Uuid) {
        if self.state.phase_of(id).is_none() {
            self.recordings.remove(&id);
        }
    }

    async fn perform_insert(&mut self, id: Uuid, text: String) {
        let context = self.recordings.remove(&id);

        // アクセシビリティ権限が無いと前面アプリ取得・キー送信のいずれもできない
        // (実装計画: 「未許可のまま挿入を試みた場合は insert_failed{reason:"accessibility"}」)。
        let ax_trusted = accessibility::is_trusted(false);
        self.status.set_ax_trusted(&self.app, ax_trusted);
        if !ax_trusted {
            tracing::warn!("アクセシビリティ権限が未許可のため挿入をスキップした");
            self.emit_insert_failed(id, "accessibility", "アクセシビリティ権限が未許可のため挿入できなかった");
            self.finish_insert(id);
            return;
        }

        let front_app = context
            .as_ref()
            .map(|c| c.front_app.clone())
            .unwrap_or_else(current_front_app_or_unknown);
        let wait = insert::paste_wait_for_app(&front_app.app_id, &self.settings.paste_wait_overrides);

        let result = match self.settings.insert_method {
            InsertMethod::ClipboardPaste => insert::insert_via_clipboard(&text, &front_app, wait).await,
            InsertMethod::DirectType => insert::insert_via_direct_type(&text, &front_app).await,
        };

        match result {
            Ok(insert::InsertOutcome::Inserted) => {
                self.history.lock().await.mark_inserted(&id.to_string());
                // design.md §5.6 (既定オフ)。クリップボード貼り付け完了後のみ対応
                // (直接送出は enigo が文字を打ち終えた時点でカーソル位置しか分からず、
                // 貼り付けと同じ「範囲選択」の意味を持たせられないため対象外)。
                if self.settings.select_after_insert
                    && matches!(self.settings.insert_method, InsertMethod::ClipboardPaste)
                {
                    // UTF-16 コード単位ではなく書記素/文字数 (insert::select_inserted_text のドキュメント参照)。
                    let char_count = text.chars().count();
                    if let Err(e) = insert::select_inserted_text(char_count) {
                        tracing::warn!("挿入後の選択状態化に失敗: {e}");
                    }
                }
            }
            Ok(insert::InsertOutcome::SkippedFrontAppChanged) => {
                tracing::info!("前面アプリが押下時と異なるため挿入を見送った");
                self.emit_insert_failed(id, "front_app_changed", "前面アプリが録音開始時と異なるため挿入を見送った");
                log_on_err(
                    overlay::show_status(&self.app, overlay::OverlayStatus::Failed),
                    "オーバーレイ状態の更新に失敗",
                );
            }
            Err(e) => {
                tracing::error!("挿入に失敗: {e}");
                self.emit_insert_failed(id, "other", &e.to_string());
                log_on_err(
                    overlay::show_status(&self.app, overlay::OverlayStatus::Failed),
                    "オーバーレイ状態の更新に失敗",
                );
            }
        }

        self.finish_insert(id);
    }

    /// UI 契約 `voice://insert_failed`。
    fn emit_insert_failed(&self, id: Uuid, reason: &'static str, message: &str) {
        let payload = InsertFailedPayload {
            id: id.to_string(),
            reason,
            message: message.to_string(),
        };
        if let Err(e) = self.app.emit(INSERT_FAILED_EVENT, payload) {
            tracing::warn!("{INSERT_FAILED_EVENT} イベントの発行に失敗: {e}");
        }
    }

    /// 挿入処理 (成功・見送り・失敗のいずれでも) の終わりに必ず行う後処理。
    fn finish_insert(&mut self, id: Uuid) {
        self.state.on_insert_finished(id);
        if self.state.is_idle() {
            log_on_err(overlay::hide(&self.app), "オーバーレイの非表示に失敗");
        }
    }
}

fn current_front_app_or_unknown() -> FrontApp {
    // 実 I/O (`insert::current_front_app` 相当) は insert モジュール内 private のため、
    // ここでは押下時点のスナップショットを insert モジュール経由で取得する薄いラッパーを使う。
    insert::snapshot_front_app().unwrap_or_else(|_| FrontApp {
        app_id: "unknown".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_wait_timeout_adds_5s_plus_10_percent() {
        let ten_seconds = Duration::from_secs(10);
        let timeout = final_wait_timeout(ten_seconds);
        assert_eq!(timeout, Duration::from_secs(5) + Duration::from_millis(1000));
    }

    #[test]
    fn final_wait_timeout_for_zero_length_utterance_is_just_5s() {
        assert_eq!(final_wait_timeout(Duration::ZERO), Duration::from_secs(5));
    }

    fn fresh_route() -> StdMutex<AudioRoute> {
        StdMutex::new(AudioRoute {
            preroll: PreRollBuffer::new(),
            current_session: None,
        })
    }

    /// レビュー指摘2 の再現テスト: 録音していない間 (current_session = None) は
    /// プリロールへ積むだけで、どこにも配送されないこと。
    #[test]
    fn route_audio_chunk_only_fills_preroll_when_idle() {
        let route = fresh_route();
        let (tx, mut rx) = mpsc::unbounded_channel();

        route_audio_chunk(&route, &tx, vec![1, 2, 3]);

        assert!(rx.try_recv().is_err(), "録音していないのに配送されてしまった");
        drop(tx);
    }

    /// 押下 (`snapshot_and_start_routing`) の後は、以後のチャンクが `AudioChunk` として
    /// 配送されること。
    #[test]
    fn route_audio_chunk_delivers_to_current_session_after_press() {
        let route = fresh_route();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = Uuid::new_v4();

        // 録音開始前に少し貯まっていたプリロール
        route_audio_chunk(&route, &tx, vec![0u8; 10]);

        let preroll = snapshot_and_start_routing(&route, id);
        assert_eq!(preroll.len(), 10, "押下前に貯まっていたプリロールが返るはず");

        route_audio_chunk(&route, &tx, vec![9, 9, 9]);
        match rx.try_recv() {
            Ok(OrchestratorEvent::AudioChunk(got_id, bytes)) => {
                assert_eq!(got_id, id);
                assert_eq!(bytes, vec![9, 9, 9]);
            }
            other => panic!("AudioChunk を期待したが {other:?}"),
        }
    }

    /// 解放 (`stop_routing`) の後は、また配送が止まってプリロールへ積むだけになること。
    #[test]
    fn stop_routing_stops_delivery_but_keeps_filling_preroll() {
        let route = fresh_route();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = Uuid::new_v4();

        snapshot_and_start_routing(&route, id);
        route_audio_chunk(&route, &tx, vec![1]);
        assert!(rx.try_recv().is_ok(), "録音中は配送されるはず");

        stop_routing(&route);
        route_audio_chunk(&route, &tx, vec![2]);
        assert!(rx.try_recv().is_err(), "解放後は配送されないはず");

        // それでもプリロールには積まれ続けている。
        let snapshot = {
            let r = route.lock().unwrap();
            r.preroll.snapshot()
        };
        assert!(!snapshot.is_empty());
    }
}
