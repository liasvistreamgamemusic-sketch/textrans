//! 常時接続の WebSocket クライアント (design.md §3.1, §5.4)。
//!
//! - `wss://<host>:8765/v1/dictate` に接続し、30秒ごとに ping する。
//! - 切断時は再接続を試み、繋がるまで音声チャンクを最大120秒分バッファする。
//! - 証明書は [`verifier::FingerprintVerifier`] で SHA-256 フィンガープリントをピン留めする。

pub mod verifier;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, Notify};
use tokio::time::interval;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::protocol::{ClientMessage, ServerMessage};

/// 常時接続の ping 間隔 (design.md §3.1「30秒ごとにping」)。
pub const PING_INTERVAL: Duration = Duration::from_secs(30);

/// pong 監視のタイムアウト。この間 pong (または他の受信) が無ければ接続死んだとみなす
/// (PING_INTERVAL の2倍、§8 レビュー指摘5)。
pub const PONG_TIMEOUT: Duration = Duration::from_secs(60);

/// 未接続時にバッファする音声の最大秒数 (design.md §5.4)。
pub const MAX_BUFFERED_AUDIO_SECONDS: u64 = 120;

/// PCM16LE モノラル 16kHz の 1 サンプルあたりバイト数。
const BYTES_PER_SAMPLE: u64 = 2;
const SAMPLE_RATE_HZ: u64 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("WebSocket I/O エラー: {0}")]
    Protocol(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("メッセージの JSON 変換に失敗: {0}")]
    Json(#[from] serde_json::Error),
    #[error("接続が相手側からクローズされた")]
    ClosedByPeer,
    #[error("{0:?} 応答が無く、接続を切って再接続する")]
    PongTimeout(Duration),
}

/// クライアント→サーバーへ送る1フレーム。テキスト (JSON) とバイナリ (PCM) を統一的に扱う。
#[derive(Debug, Clone)]
pub enum OutgoingFrame {
    Control(ClientMessage),
    Pcm(Vec<u8>),
}

impl OutgoingFrame {
    fn byte_len(&self) -> usize {
        match self {
            OutgoingFrame::Control(_) => 0,
            OutgoingFrame::Pcm(bytes) => bytes.len(),
        }
    }
}

/// 未接続中・接続中を問わず送信フレームを溜めておくバッファ。上限を超えたら古いチャンクから
/// 捨てる (無限に溜め続けてメモリを圧迫しないため。上限は design.md §5.4 の「最大120秒」)。
/// [`PendingQueue`] がこれを実体として持ち、接続の有無に関わらず全ての送信がここを通る。
pub struct AudioSendBuffer {
    max_bytes: usize,
    buffered_bytes: usize,
    frames: VecDeque<OutgoingFrame>,
}

impl AudioSendBuffer {
    pub fn new() -> Self {
        Self::with_max_seconds(MAX_BUFFERED_AUDIO_SECONDS)
    }

    pub fn with_max_seconds(seconds: u64) -> Self {
        let max_bytes = (seconds * SAMPLE_RATE_HZ * BYTES_PER_SAMPLE) as usize;
        Self {
            max_bytes,
            buffered_bytes: 0,
            frames: VecDeque::new(),
        }
    }

    pub fn push(&mut self, frame: OutgoingFrame) {
        self.buffered_bytes += frame.byte_len();
        self.frames.push_back(frame);
        while self.buffered_bytes > self.max_bytes {
            match self.frames.pop_front() {
                Some(dropped) => self.buffered_bytes -= dropped.byte_len(),
                None => break,
            }
        }
    }

    /// 先頭から1件だけ取り出す (FIFO)。空なら `None`。
    pub fn pop_front(&mut self) -> Option<OutgoingFrame> {
        let frame = self.frames.pop_front()?;
        self.buffered_bytes -= frame.byte_len();
        Some(frame)
    }

    pub fn drain(&mut self) -> Vec<OutgoingFrame> {
        self.buffered_bytes = 0;
        self.frames.drain(..).collect()
    }

    pub fn buffered_bytes(&self) -> usize {
        self.buffered_bytes
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }
}

impl Default for AudioSendBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// 未接続時・再接続中を含め常時容量キャップを適用する送信キュー。
/// [`Orchestrator`](crate::orchestrator::Orchestrator) はここへ直接 (同期的に) 積むだけでよく、
/// 接続の有無・再接続のタイミングを意識する必要が無い。[`drive_connection`] が
/// 接続が張れている間ここから随時取り出して送る (レビュー指摘4: 以前は素の unbounded channel を
/// 使っていたため、未接続の間キャップが一切効かなかった)。
#[derive(Clone)]
pub struct PendingQueue {
    buffer: Arc<StdMutex<AudioSendBuffer>>,
    notify: Arc<Notify>,
}

impl PendingQueue {
    pub fn new() -> Self {
        Self {
            buffer: Arc::new(StdMutex::new(AudioSendBuffer::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 同期的に積む。cpal のコールバックスレッド (非 async) からも直接呼べる。
    pub fn push(&self, frame: OutgoingFrame) {
        self.lock().push(frame);
        self.notify.notify_one();
    }

    /// 何か積まれるまで待って1件取り出す。
    pub async fn pop(&self) -> OutgoingFrame {
        loop {
            // 先に Notified を作ってから中身を見る (取り忘れ防止: この後の push はここで捕まえられる)。
            let notified = self.notify.notified();
            if let Some(frame) = self.lock().pop_front() {
                return frame;
            }
            notified.await;
        }
    }

    pub fn buffered_bytes(&self) -> usize {
        self.lock().buffered_bytes()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AudioSendBuffer> {
        self.buffer.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for PendingQueue {
    fn default() -> Self {
        Self::new()
    }
}

fn to_ws_message(frame: OutgoingFrame) -> Result<Message, WsError> {
    match frame {
        OutgoingFrame::Control(msg) => Ok(Message::text(serde_json::to_string(&msg)?)),
        OutgoingFrame::Pcm(bytes) => Ok(Message::binary(bytes)),
    }
}

/// 確立済みの WebSocket 接続に対して、1本の発話サイクル分のやり取りをする低レベル関数。
/// 上位の再接続・バッファリングは呼び出し側 (このモジュールの `drive_connection`) が持つ。
pub async fn send_frame<S>(ws: &mut WebSocketStream<S>, frame: OutgoingFrame) -> Result<(), WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    ws.send(to_ws_message(frame)?).await?;
    Ok(())
}

pub async fn send_ping<S>(ws: &mut WebSocketStream<S>) -> Result<(), WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    ws.send(Message::Ping(Vec::new().into())).await?;
    Ok(())
}

/// `next_ws_event` が返す生のイベント。pong を透過処理する [`recv_server_message`] と、
/// pong を生存監視に使う [`drive_connection`] の両方から使う (レビュー指摘5)。
enum WsEvent {
    Server(ServerMessage),
    Pong,
    Closed,
}

async fn next_ws_event<S>(ws: &mut WebSocketStream<S>) -> Result<WsEvent, WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                let parsed = serde_json::from_str::<ServerMessage>(text.as_str())?;
                return Ok(WsEvent::Server(parsed));
            }
            Some(Ok(Message::Pong(_))) => return Ok(WsEvent::Pong),
            Some(Ok(Message::Ping(_) | Message::Frame(_))) => continue,
            Some(Ok(Message::Binary(_))) => {
                tracing::warn!("サーバーから予期しないバイナリフレームを受信。無視する");
                continue;
            }
            Some(Ok(Message::Close(_))) | None => return Ok(WsEvent::Closed),
            Some(Err(e)) => return Err(e.into()),
        }
    }
}

/// 次の S→C メッセージを1つ受け取る。ping/pong/close フレームは透過的に処理する。
/// `Ok(None)` は相手が正常にクローズしたことを示す。
pub async fn recv_server_message<S>(
    ws: &mut WebSocketStream<S>,
) -> Result<Option<ServerMessage>, WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        match next_ws_event(ws).await? {
            WsEvent::Server(msg) => return Ok(Some(msg)),
            WsEvent::Pong => continue,
            WsEvent::Closed => return Ok(None),
        }
    }
}

/// 1本の確立済み接続を、切断されるまで駆動する ([`PendingQueue`] からの送信 + 30秒ping +
/// pong 監視 + 受信転送)。呼び出し側は切断を検知したら (Err/Ok(())) 再接続して呼び直す。
/// `pending` はここに渡された時点で溜まっている分もそのまま送信対象になる
/// (未接続中もキャップ付きで溜まり続けるため、接続直後の特別な flush 処理は不要)。
pub async fn drive_connection<S>(
    ws: WebSocketStream<S>,
    pending: &PendingQueue,
    incoming_tx: &mpsc::UnboundedSender<ServerMessage>,
) -> Result<(), WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    drive_connection_with_timing(ws, pending, incoming_tx, PING_INTERVAL, PONG_TIMEOUT).await
}

/// `drive_connection` の本体。ping間隔/pongタイムアウトを外から差し替えられるようにして、
/// テストで実際の 30秒/60秒を待たずに同じロジックを検証できるようにする。
async fn drive_connection_with_timing<S>(
    mut ws: WebSocketStream<S>,
    pending: &PendingQueue,
    incoming_tx: &mpsc::UnboundedSender<ServerMessage>,
    ping_interval: Duration,
    pong_timeout: Duration,
) -> Result<(), WsError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut ping_tick = interval(ping_interval);
    ping_tick.tick().await; // 最初の tick は即時なので消費しておく
    let mut last_activity = tokio::time::Instant::now();

    loop {
        tokio::select! {
            _ = ping_tick.tick() => {
                if last_activity.elapsed() > pong_timeout {
                    return Err(WsError::PongTimeout(pong_timeout));
                }
                send_ping(&mut ws).await?;
            }
            frame = pending.pop() => {
                send_frame(&mut ws, frame).await?;
            }
            event = next_ws_event(&mut ws) => {
                match event? {
                    WsEvent::Server(msg) => {
                        last_activity = tokio::time::Instant::now();
                        if incoming_tx.send(msg).is_err() {
                            return Ok(()); // 受信側がドロップ = アプリ終了
                        }
                    }
                    WsEvent::Pong => {
                        last_activity = tokio::time::Instant::now();
                    }
                    WsEvent::Closed => return Err(WsError::ClosedByPeer),
                }
            }
        }
    }
}

/// 初回接続 (TOFU) 用: 実際の検証は行わず、サーバー証明書のフィンガープリントだけを取得する
/// (design.md §3.1, §5.8「初回接続時に表示し、ユーザーがサーバー側の値と照合して承認する」)。
/// 取得後、接続は張ったままにせず切断する。
pub async fn probe_fingerprint(host: &str, port: u16) -> Result<String, WsError> {
    use std::sync::Arc;

    let tcp = tokio::net::TcpStream::connect((host, port))
        .await
        .map_err(|e| WsError::Protocol(tokio_tungstenite::tungstenite::Error::Io(e)))?;
    let (capturing, captured) = verifier::CapturingVerifier::new();
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(capturing))
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| WsError::Protocol(tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(e))))?;
    let _tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| WsError::Protocol(tokio_tungstenite::tungstenite::Error::Io(std::io::Error::other(e))))?;

    let value = captured
        .lock()
        .map_err(|_| WsError::ClosedByPeer)?
        .clone();
    value.ok_or(WsError::ClosedByPeer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Mode;
    use rcgen::{generate_simple_self_signed, CertifiedKey};
    use rustls::pki_types::{PrivatePkcs8KeyDer, ServerName};
    use rustls::{ClientConfig, ServerConfig};
    use std::sync::Arc;
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};
    use uuid::Uuid;

    fn audio_buffer_max_bytes() -> usize {
        (MAX_BUFFERED_AUDIO_SECONDS * SAMPLE_RATE_HZ * BYTES_PER_SAMPLE) as usize
    }

    #[test]
    fn audio_buffer_drops_oldest_when_over_capacity() {
        let mut buf = AudioSendBuffer::with_max_seconds(1); // 32,000 バイト上限
        buf.push(OutgoingFrame::Pcm(vec![0u8; 20_000]));
        buf.push(OutgoingFrame::Pcm(vec![1u8; 20_000]));
        // 40,000 > 32,000 なので最初のチャンクは捨てられているはず
        assert!(buf.buffered_bytes() <= 32_000);
        let drained = buf.drain();
        assert_eq!(drained.len(), 1);
        match &drained[0] {
            OutgoingFrame::Pcm(bytes) => assert_eq!(bytes[0], 1),
            OutgoingFrame::Control(_) => panic!("PCM のはず"),
        }
    }

    #[test]
    fn audio_buffer_default_matches_120_seconds() {
        let buf = AudioSendBuffer::new();
        assert_eq!(buf.max_bytes, audio_buffer_max_bytes());
    }

    #[test]
    fn audio_buffer_drain_empties_it() {
        let mut buf = AudioSendBuffer::new();
        buf.push(OutgoingFrame::Pcm(vec![0u8; 100]));
        assert!(!buf.is_empty());
        let drained = buf.drain();
        assert_eq!(drained.len(), 1);
        assert!(buf.is_empty());
        assert_eq!(buf.buffered_bytes(), 0);
    }

    #[test]
    fn audio_buffer_pop_front_is_fifo_and_updates_byte_count() {
        let mut buf = AudioSendBuffer::new();
        buf.push(OutgoingFrame::Pcm(vec![1u8; 10]));
        buf.push(OutgoingFrame::Pcm(vec![2u8; 20]));
        assert_eq!(buf.buffered_bytes(), 30);

        let first = buf.pop_front().expect("1件目");
        match first {
            OutgoingFrame::Pcm(bytes) => assert_eq!(bytes, vec![1u8; 10]),
            OutgoingFrame::Control(_) => panic!("PCM のはず"),
        }
        assert_eq!(buf.buffered_bytes(), 20);

        let second = buf.pop_front().expect("2件目");
        match second {
            OutgoingFrame::Pcm(bytes) => assert_eq!(bytes, vec![2u8; 20]),
            OutgoingFrame::Control(_) => panic!("PCM のはず"),
        }
        assert_eq!(buf.buffered_bytes(), 0);
        assert!(buf.pop_front().is_none());
    }

    /// レビュー指摘4の再現テスト: 未接続中 (`drive_connection` を一度も呼ばない) に
    /// 120秒を超える音声を積んでも、`PendingQueue` 自体が古いものから捨てるので
    /// 無制限に膨らまないことを示す。以前は素の unbounded mpsc に直接積んでいたため、
    /// このキャップが一切効いていなかった。
    #[test]
    fn pending_queue_caps_audio_while_disconnected() {
        let queue = PendingQueue::new();
        let max_bytes = (MAX_BUFFERED_AUDIO_SECONDS * SAMPLE_RATE_HZ * BYTES_PER_SAMPLE) as usize;

        // 120秒 + α 分の 100ms チャンクを、一度も pop せずに積み続ける (=未接続を再現)。
        let chunk = vec![0u8; 3_200]; // 100ms 分
        let total_chunks = (max_bytes / chunk.len()) + 50;
        for _ in 0..total_chunks {
            queue.push(OutgoingFrame::Pcm(chunk.clone()));
        }

        assert!(
            queue.buffered_bytes() <= max_bytes,
            "120秒キャップを超えて溜まっている: {} > {}",
            queue.buffered_bytes(),
            max_bytes
        );
    }

    #[tokio::test]
    async fn pending_queue_pop_returns_in_fifo_order() {
        let queue = PendingQueue::new();
        queue.push(OutgoingFrame::Pcm(vec![1]));
        queue.push(OutgoingFrame::Pcm(vec![2]));

        let first = queue.pop().await;
        let second = queue.pop().await;
        assert!(matches!(first, OutgoingFrame::Pcm(b) if b == vec![1]));
        assert!(matches!(second, OutgoingFrame::Pcm(b) if b == vec![2]));
    }

    #[tokio::test]
    async fn pending_queue_pop_waits_until_something_is_pushed() {
        let queue = PendingQueue::new();
        let popped = std::sync::Arc::new(tokio::sync::Mutex::new(false));

        let queue_clone = queue.clone();
        let popped_clone = popped.clone();
        let task = tokio::spawn(async move {
            queue_clone.pop().await;
            *popped_clone.lock().await = true;
        });

        // まだ何も積んでいないので、少し待っても pop は完了していないはず。
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!*popped.lock().await, "push する前に pop が完了してしまった");

        queue.push(OutgoingFrame::Pcm(vec![9]));
        task.await.expect("pop タスク");
        assert!(*popped.lock().await, "push 後に pop が完了するはず");
    }

    /// レビュー指摘5の再現テスト: pong (も他の受信も) が `PONG_TIMEOUT` の間無ければ、
    /// `drive_connection` がエラーで抜けて呼び出し側が再接続できるようにする。
    #[tokio::test]
    async fn drive_connection_gives_up_after_pong_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("自己署名証明書の生成");
        let cert_der = cert.der().clone();
        let fingerprint = verifier::fingerprint_hex(&cert_der);
        let key_der = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("サーバー TLS 設定");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let tls = acceptor.accept(tcp).await.expect("tls accept");
            let ws = tokio_tungstenite::accept_async(tls).await.expect("ws accept");
            // サーバー側は何も送らずただ持ち続ける (pong も送らない = クライアントを孤立させる)。
            tokio::time::sleep(Duration::from_secs(3)).await;
            drop(ws);
        });

        let tcp = TcpStream::connect(addr).await.expect("client connect");
        let client_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(
                verifier::FingerprintVerifier::new(&fingerprint).expect("valid fingerprint format"),
            ))
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));
        let server_name = ServerName::try_from("localhost").unwrap();
        let tls_stream = connector.connect(server_name, tcp).await.expect("tls connect");
        let (ws, _response) = tokio_tungstenite::client_async("wss://localhost/v1/dictate", tls_stream)
            .await
            .expect("client handshake");

        let pending = PendingQueue::new();
        let (incoming_tx, _incoming_rx) = mpsc::unbounded_channel();

        // 実際の PING_INTERVAL/PONG_TIMEOUT (30秒/60秒) をテストで待つのは非現実的なので、
        // 本体 (`drive_connection_with_timing`) はそのまま使い、間隔だけ短く差し替える。
        let result = drive_connection_with_timing(
            ws,
            &pending,
            &incoming_tx,
            Duration::from_millis(50),
            Duration::from_millis(150),
        )
        .await;
        assert!(matches!(result, Err(WsError::PongTimeout(_))));

        server_task.abort();
    }

    /// start → PCM → end → final の往復を、TLS 込みのローカルモックサーバーで確認する。
    /// 証明書はテスト用に自己署名し、クライアントは実サーバー同様
    /// `FingerprintVerifier` でその SHA-256 をピン留めする (design.md §3.1)。
    #[tokio::test]
    async fn start_pcm_end_final_round_trip_over_pinned_tls() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["localhost".to_string()]).expect("自己署名証明書の生成");
        let cert_der = cert.der().clone();
        let fingerprint = verifier::fingerprint_hex(&cert_der);
        let key_der = PrivatePkcs8KeyDer::from(signing_key.serialize_der());

        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der.into())
            .expect("サーバー TLS 設定");
        let acceptor = TlsAcceptor::from(Arc::new(server_config));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.expect("accept");
            let tls = acceptor.accept(tcp).await.expect("tls accept");
            let mut ws = tokio_tungstenite::accept_async(tls).await.expect("ws accept");

            let session_id = match recv_server_side(&mut ws).await {
                ClientMessage::Start { session_id, .. } => session_id,
                other => panic!("start を期待したが {other:?}"),
            };

            // PCM チャンクが1つ届くはず
            let pcm = match ws.next().await {
                Some(Ok(Message::Binary(bytes))) => bytes.to_vec(),
                other => panic!("PCM バイナリフレームを期待したが {other:?}"),
            };
            assert_eq!(pcm.len(), 3_200);

            match recv_server_side(&mut ws).await {
                ClientMessage::End { session_id: end_id } => assert_eq!(end_id, session_id),
                other => panic!("end を期待したが {other:?}"),
            }

            let final_msg = ServerMessage::Final {
                session_id,
                raw_text: "テスト".to_string(),
                text: "テスト。".to_string(),
                mode: Mode::Clean,
                flags: vec![],
                timings: crate::protocol::Timings {
                    asr_ms: 1,
                    llm_ms: 1,
                    total_ms: 2,
                },
            };
            ws.send(Message::text(serde_json::to_string(&final_msg).unwrap()))
                .await
                .expect("final 送信");
        });

        let tcp = TcpStream::connect(addr).await.expect("client connect");
        let client_config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(
                verifier::FingerprintVerifier::new(&fingerprint).expect("valid fingerprint format"),
            ))
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));
        let server_name = ServerName::try_from("localhost").unwrap();
        let tls_stream = connector.connect(server_name, tcp).await.expect("tls connect");

        let (mut ws, _response) = tokio_tungstenite::client_async("wss://localhost/v1/dictate", tls_stream)
            .await
            .expect("client handshake");

        let session_id = Uuid::new_v4();
        send_frame(
            &mut ws,
            OutgoingFrame::Control(ClientMessage::start(session_id, Mode::Clean)),
        )
        .await
        .expect("start 送信");
        send_frame(&mut ws, OutgoingFrame::Pcm(vec![0u8; 3_200]))
            .await
            .expect("pcm 送信");
        send_frame(&mut ws, OutgoingFrame::Control(ClientMessage::end(session_id)))
            .await
            .expect("end 送信");

        let final_msg = recv_server_message(&mut ws)
            .await
            .expect("final 受信")
            .expect("接続がクローズされていないこと");

        match final_msg {
            ServerMessage::Final { session_id: got_id, text, .. } => {
                assert_eq!(got_id, session_id);
                assert_eq!(text, "テスト。");
            }
            other => panic!("final を期待したが {other:?}"),
        }

        server_task.await.expect("server task panicked");
    }

    async fn recv_server_side<S>(ws: &mut WebSocketStream<S>) -> ClientMessage
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                serde_json::from_str(text.as_str()).expect("valid ClientMessage")
            }
            other => panic!("テキストフレームを期待したが {other:?}"),
        }
    }
}
