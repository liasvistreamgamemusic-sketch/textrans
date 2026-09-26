// サーバー証明書フィンガープリントの正規化 (design.md §3.1, §5.8)。
//
// `voice-server cert fingerprint` はコロン区切り・大文字 (例 "8C:4D:17:...") で表示するが、
// クライアント側の計算値 (`fingerprint_hex` / `probe_fingerprint`) はコロン無し・小文字。
// ユーザーが手動で貼り付けたときも同じ形へ揃える。
// Rust 側の `normalize_fingerprint` (src-tauri/src/ws/verifier.rs) と同じ規則:
// コロン・空白 (前後含む) を除去し、64桁の16進数であることを検証してから小文字化する。

export class FingerprintFormatError extends Error {}

export function normalizeFingerprint(input: string): string {
  const cleaned = input.replace(/[\s:]/g, "");
  if (cleaned.length !== 64) {
    throw new FingerprintFormatError(
      `フィンガープリントは64桁の16進数でなければならない (コロン・空白を除いた実際の文字数: ${cleaned.length})`,
    );
  }
  if (!/^[0-9a-fA-F]{64}$/.test(cleaned)) {
    throw new FingerprintFormatError("フィンガープリントに16進数以外の文字が含まれている");
  }
  return cleaned.toLowerCase();
}

// 表示用: 内部形式 (コロン無し・小文字) を `voice-server cert fingerprint` と同じ
// コロン区切り・大文字表記に変換する (ペアリング成功時の表示など)。
export function formatFingerprintColonUppercase(input: string): string {
  const normalized = normalizeFingerprint(input);
  const pairs: string[] = [];
  for (let i = 0; i < normalized.length; i += 2) {
    pairs.push(normalized.slice(i, i + 2).toUpperCase());
  }
  return pairs.join(":");
}
