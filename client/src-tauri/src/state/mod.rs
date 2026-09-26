//! クライアントの状態機械 (design.md §5.2, §5.3)。
//!
//! 発話ごとに `Recording → Waiting → Inserting` (または `Idle` へ抜ける経路) を辿る。
//! 複数の発話が並行して進みうる (処理待ち中でも次を録音できる) ため、
//! 状態は発話 (`Uuid` をキーにした `Session`) の集合として持ち、
//! 挿入は「発話した順」を守るキュー (`order`) で直列化する。
//!
//! ホットキーのリピート押下 (押しっぱなしで OS が繰り返し発火する press イベント) は無視する。

use std::collections::{HashMap, VecDeque};

use uuid::Uuid;

use crate::protocol::Mode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// 録音中。ホットキー押下〜解放/Escまで。
    Recording,
    /// `end` 送信済みで `final` を待っている。
    WaitingFinal,
    /// `final` を受信し、挿入可能なテキストを持っている。
    ReadyToInsert { text: String },
    /// 挿入処理を実行中。
    Inserting { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub mode: Mode,
    pub phase: Phase,
}

/// `on_final_received` / `on_error_or_timeout` の結果、呼び出し側が実際に行うべきこと。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 何もしない (該当セッションが無い、またはまだ順番待ち)。
    None,
    /// このセッションのテキストを挿入する (発話順が来たもの)。
    Insert { id: Uuid, text: String },
}

/// 押下/解放イベントの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressOutcome {
    /// 新しい発話を開始した (`start` を送るべき)。
    Started(Uuid),
    /// リピート押下として無視した。
    IgnoredRepeat,
}

#[derive(Debug, Default)]
pub struct StateMachine {
    order: VecDeque<Uuid>,
    sessions: HashMap<Uuid, Session>,
    hotkey_pressed: bool,
}

impl StateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// ホットキー押下。リピートイベント (すでに押されている間の再発火) は無視する。
    pub fn on_hotkey_press(&mut self, mode: Mode) -> PressOutcome {
        if self.hotkey_pressed {
            return PressOutcome::IgnoredRepeat;
        }
        self.hotkey_pressed = true;
        let id = Uuid::new_v4();
        self.order.push_back(id);
        self.sessions.insert(
            id,
            Session {
                id,
                mode,
                phase: Phase::Recording,
            },
        );
        PressOutcome::Started(id)
    }

    /// ホットキー解放。録音中のセッションを `end` 待ちへ進める。
    /// 対応するセッションが無ければ `None` (すでに Esc 等で消えている等)。
    pub fn on_hotkey_release(&mut self) -> Option<Uuid> {
        self.hotkey_pressed = false;
        let recording_id = self
            .order
            .iter()
            .rev()
            .find(|id| matches!(self.sessions.get(*id).map(|s| &s.phase), Some(Phase::Recording)))
            .copied()?;
        if let Some(session) = self.sessions.get_mut(&recording_id) {
            session.phase = Phase::WaitingFinal;
        }
        Some(recording_id)
    }

    /// Esc による取り消し。録音中のセッションを破棄する (`cancel` を送るべき)。
    pub fn on_cancel(&mut self) -> Option<Uuid> {
        self.hotkey_pressed = false;
        let recording_id = self
            .order
            .iter()
            .rev()
            .find(|id| matches!(self.sessions.get(*id).map(|s| &s.phase), Some(Phase::Recording)))
            .copied()?;
        self.remove(recording_id);
        Some(recording_id)
    }

    /// `final` 受信。空文字なら挿入せず即座に破棄する (§5.3 `Waiting -> Idle: final(空)`)。
    /// 非空なら「挿入可能」の状態にし、発話順キューの先頭がこのセッションなら挿入を許可する。
    pub fn on_final_received(&mut self, id: Uuid, text: String) -> Outcome {
        if text.is_empty() {
            self.remove(id);
            return self.try_advance_front();
        }
        match self.sessions.get_mut(&id) {
            Some(session) if session.phase == Phase::WaitingFinal => {
                session.phase = Phase::ReadyToInsert { text };
            }
            _ => return Outcome::None, // 不明・タイムアウト後・重複受信
        }
        self.try_advance_front()
    }

    /// サーバーからの明確なエラー通知でこの発話を諦める。挿入は行わない。
    /// (サーバーが「この発話はもう終わり」と言っている以上、フェーズに関わらず必ず消す)
    pub fn on_error_or_timeout(&mut self, id: Uuid) -> Outcome {
        self.remove(id);
        self.try_advance_front()
    }

    /// クライアント側の `final` 待ちタイムアウト (design.md §5.4)。
    /// サーバーからの `on_error_or_timeout` とは異なり、これは「一定時間 final が来なかった」
    /// という *推測* に基づくイベントであり、実際にはその直前に final が届いていて
    /// すでに `ReadyToInsert`/`Inserting` になっている可能性がある
    /// (タイムアウト通知とサーバー応答のレース)。その場合に横取りして消してしまうと、
    /// 受信済みの結果を挿入せず捨てることになるため、`WaitingFinal` のときだけ諦める。
    pub fn on_timeout(&mut self, id: Uuid) -> Outcome {
        match self.sessions.get(&id).map(|s| &s.phase) {
            Some(Phase::WaitingFinal) => self.on_error_or_timeout(id),
            _ => Outcome::None,
        }
    }

    /// 挿入完了 (成功・前面ウィンドウ不一致による見送りの両方) の通知。セッションを終了させる。
    pub fn on_insert_finished(&mut self, id: Uuid) {
        self.remove(id);
    }

    /// 発話順キューの先頭が「挿入可能」なら Inserting に進めて返す。まだなら `Outcome::None`。
    fn try_advance_front(&mut self) -> Outcome {
        let front_id = match self.order.front() {
            Some(id) => *id,
            None => return Outcome::None,
        };
        let session = match self.sessions.get_mut(&front_id) {
            Some(s) => s,
            None => return Outcome::None,
        };
        match &session.phase {
            Phase::ReadyToInsert { text } => {
                let text = text.clone();
                session.phase = Phase::Inserting { text: text.clone() };
                Outcome::Insert { id: front_id, text }
            }
            _ => Outcome::None,
        }
    }

    fn remove(&mut self, id: Uuid) {
        self.sessions.remove(&id);
        if let Some(pos) = self.order.iter().position(|x| *x == id) {
            self.order.remove(pos);
        }
    }

    pub fn phase_of(&self, id: Uuid) -> Option<&Phase> {
        self.sessions.get(&id).map(|s| &s.phase)
    }

    pub fn is_idle(&self) -> bool {
        self.sessions.is_empty()
    }

    /// オーバーレイ表示用の概況。design.md §5.1 の「録音中/処理中/失敗」に対応する粒度。
    pub fn overlay_summary(&self) -> OverlaySummary {
        if self.sessions.values().any(|s| s.phase == Phase::Recording) {
            OverlaySummary::Recording
        } else if !self.sessions.is_empty() {
            OverlaySummary::Processing
        } else {
            OverlaySummary::Idle
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlaySummary {
    Idle,
    Recording,
    Processing,
}

/// UI 契約 (`voice://status` の `phase`) 用の粒度。[`OverlaySummary`] より1段階細かく、
/// `WaitingFinal`/`ReadyToInsert` を「待機中」、`Inserting` を独立した状態として区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPhase {
    Idle,
    Recording,
    Waiting,
    Inserting,
}

impl StateMachine {
    /// UI 契約用のフェーズ。複数の発話が並行している場合は、最も目立つ状態を優先する
    /// (挿入中 > 録音中 > 待機中 > アイドル)。
    pub fn ui_phase(&self) -> UiPhase {
        if self.sessions.values().any(|s| matches!(s.phase, Phase::Inserting { .. })) {
            UiPhase::Inserting
        } else if self.sessions.values().any(|s| s.phase == Phase::Recording) {
            UiPhase::Recording
        } else if !self.sessions.is_empty() {
            UiPhase::Waiting
        } else {
            UiPhase::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_press_is_ignored_while_held() {
        let mut sm = StateMachine::new();
        let first = sm.on_hotkey_press(Mode::Clean);
        assert!(matches!(first, PressOutcome::Started(_)));
        let repeat = sm.on_hotkey_press(Mode::Clean);
        assert_eq!(repeat, PressOutcome::IgnoredRepeat);
    }

    #[test]
    fn press_after_release_starts_new_session() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(first_id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!("started のはず")
        };
        sm.on_hotkey_release();
        let second = sm.on_hotkey_press(Mode::Clean);
        match second {
            PressOutcome::Started(second_id) => assert_ne!(first_id, second_id),
            PressOutcome::IgnoredRepeat => panic!("解放後は新規開始のはず"),
        }
    }

    #[test]
    fn cancel_removes_recording_session() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        let cancelled = sm.on_cancel();
        assert_eq!(cancelled, Some(id));
        assert!(sm.is_idle());
    }

    #[test]
    fn empty_final_does_not_insert() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        let outcome = sm.on_final_received(id, String::new());
        assert_eq!(outcome, Outcome::None);
        assert!(sm.is_idle());
    }

    #[test]
    fn single_session_inserts_when_final_arrives() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        let outcome = sm.on_final_received(id, "こんにちは".to_string());
        assert_eq!(
            outcome,
            Outcome::Insert {
                id,
                text: "こんにちは".to_string()
            }
        );
    }

    /// 2つの発話が並行していて、後から始めた方の final が先に届いても、
    /// 先に始めた発話の挿入が終わるまで割り込ませてはならない (design.md §5.2「発話した順に挿入」)。
    #[test]
    fn insertion_order_follows_utterance_start_order_not_final_arrival_order() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(first_id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        let PressOutcome::Started(second_id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();

        // 2番目の発話の final が先に届く
        let outcome = sm.on_final_received(second_id, "2番目".to_string());
        assert_eq!(outcome, Outcome::None, "1番目がまだなので挿入してはいけない");

        // 1番目の final が届いたら、1番目から順に挿入できる
        let outcome = sm.on_final_received(first_id, "1番目".to_string());
        assert_eq!(
            outcome,
            Outcome::Insert {
                id: first_id,
                text: "1番目".to_string()
            }
        );

        // 1番目の挿入完了を通知すると、2番目が続けて挿入可能になる
        sm.on_insert_finished(first_id);
        // フロントが進んだことを確認するため、明示的に再チェックする
        let front_ready = sm.order.front().copied();
        assert_eq!(front_ready, Some(second_id));
    }

    #[test]
    fn error_or_timeout_drops_session_without_insert() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        let outcome = sm.on_error_or_timeout(id);
        assert_eq!(outcome, Outcome::None);
        assert!(sm.is_idle());
    }

    #[test]
    fn timeout_drops_session_that_is_still_waiting_for_final() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        assert_eq!(sm.phase_of(id), Some(&Phase::WaitingFinal));

        let outcome = sm.on_timeout(id);
        assert_eq!(outcome, Outcome::None);
        assert!(sm.is_idle(), "WaitingFinal のままタイムアウトしたら消えるはず");
    }

    /// レビュー指摘3の再現テスト: 2番目の発話が (1番目がまだ WaitingFinal のため)
    /// ReadyToInsert のまま順番待ちしている間にタイムアウト通知が来ても、
    /// 受信済みの結果を消してはならない。
    #[test]
    fn timeout_does_not_clobber_a_session_that_already_became_ready_to_insert() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(first) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        let PressOutcome::Started(second) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();

        // 2番目の final が先に届く (1番目がまだ WaitingFinal なので挿入はまだ)。
        let outcome = sm.on_final_received(second, "2番目".to_string());
        assert_eq!(outcome, Outcome::None);
        assert_eq!(
            sm.phase_of(second),
            Some(&Phase::ReadyToInsert { text: "2番目".to_string() })
        );

        // 2番目に対してタイムアウト通知が来ても、ReadyToInsert のまま残る。
        let timeout_outcome = sm.on_timeout(second);
        assert_eq!(timeout_outcome, Outcome::None);
        assert_eq!(
            sm.phase_of(second),
            Some(&Phase::ReadyToInsert { text: "2番目".to_string() }),
            "final 受信済みのセッションをタイムアウトで消してはいけない"
        );

        // 一方、1番目 (まだ WaitingFinal) はタイムアウトでちゃんと消える。
        // 1番目が消えたことで、順番待ちしていた2番目 (既に ReadyToInsert) が繰り上がって
        // 挿入対象になる — これは正しい挙動 (ブロックしていたセッションが無くなったので進める)。
        let first_timeout_outcome = sm.on_timeout(first);
        assert_eq!(
            first_timeout_outcome,
            Outcome::Insert { id: second, text: "2番目".to_string() }
        );
        assert!(sm.phase_of(first).is_none());
    }

    #[test]
    fn ui_phase_is_idle_when_no_sessions() {
        let sm = StateMachine::new();
        assert_eq!(sm.ui_phase(), UiPhase::Idle);
    }

    #[test]
    fn ui_phase_is_recording_while_a_session_is_recording() {
        let mut sm = StateMachine::new();
        sm.on_hotkey_press(Mode::Clean);
        assert_eq!(sm.ui_phase(), UiPhase::Recording);
    }

    #[test]
    fn ui_phase_is_waiting_after_release_before_final() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        assert_eq!(sm.phase_of(id), Some(&Phase::WaitingFinal));
        assert_eq!(sm.ui_phase(), UiPhase::Waiting);
    }

    #[test]
    fn ui_phase_is_inserting_once_advanced_to_front() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        sm.on_final_received(id, "テスト".to_string());
        assert_eq!(sm.ui_phase(), UiPhase::Inserting);
    }

    #[test]
    fn ui_phase_prioritizes_recording_over_waiting_when_both_present() {
        let mut sm = StateMachine::new();
        let PressOutcome::Started(first) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        sm.on_hotkey_release();
        assert_eq!(sm.phase_of(first), Some(&Phase::WaitingFinal));
        sm.on_hotkey_press(Mode::Clean); // 2番目を録音中に
        assert_eq!(sm.ui_phase(), UiPhase::Recording);
    }

    #[test]
    fn overlay_summary_reflects_current_phase() {
        let mut sm = StateMachine::new();
        assert_eq!(sm.overlay_summary(), OverlaySummary::Idle);
        let PressOutcome::Started(id) = sm.on_hotkey_press(Mode::Clean) else {
            panic!()
        };
        assert_eq!(sm.overlay_summary(), OverlaySummary::Recording);
        sm.on_hotkey_release();
        assert_eq!(sm.overlay_summary(), OverlaySummary::Processing);
        sm.on_error_or_timeout(id);
        assert_eq!(sm.overlay_summary(), OverlaySummary::Idle);
    }
}
