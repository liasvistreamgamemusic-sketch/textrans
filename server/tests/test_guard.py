from __future__ import annotations

from voice_server.guard import (
    apply_clean_guard,
    apply_translate_guard,
    japanese_char_ratio,
    strip_llm_noise,
)


def test_strip_llm_noise_removes_think_tag() -> None:
    text = "<think>考え中</think>こんにちは、元気ですか。"
    assert strip_llm_noise(text) == "こんにちは、元気ですか。"


def test_strip_llm_noise_removes_preface() -> None:
    assert strip_llm_noise("清書しました:こんにちは。") == "こんにちは。"


def test_strip_llm_noise_removes_code_fence() -> None:
    assert strip_llm_noise("```\nこんにちは。\n```") == "こんにちは。"


def test_japanese_char_ratio_all_japanese() -> None:
    assert japanese_char_ratio("こんにちは") == 1.0


def test_japanese_char_ratio_empty_is_zero() -> None:
    assert japanese_char_ratio("") == 0.0


def test_clean_guard_accepts_reasonable_output() -> None:
    raw = "えーと明日の会議の件です"
    llm_out = "明日の会議の件です。"
    result = apply_clean_guard(raw, llm_out)
    assert result.flags == []
    assert result.text == "明日の会議の件です。"


def test_clean_guard_rejects_too_short_output() -> None:
    raw = "明日の会議の件についてお伝えしたいことがあります"
    llm_out = "了解"
    result = apply_clean_guard(raw, llm_out)
    assert result.flags == ["llm_rejected"]
    assert result.text == raw


def test_clean_guard_rejects_too_long_output() -> None:
    raw = "会議です"
    llm_out = "会議です。" * 10
    result = apply_clean_guard(raw, llm_out)
    assert result.flags == ["llm_rejected"]
    assert result.text == raw


def test_clean_guard_rejects_empty_output_for_nonempty_input() -> None:
    result = apply_clean_guard("こんにちは", "")
    assert result.flags == ["llm_rejected"]
    assert result.text == "こんにちは"


def test_clean_guard_boundary_ratio_min_is_accepted() -> None:
    raw = "0123456789"  # 10文字
    llm_out = "01234"  # 5文字 = 比0.5 (>0.4 なので合格)
    result = apply_clean_guard(raw, llm_out)
    assert result.flags == []


def test_translate_guard_accepts_english_output() -> None:
    raw = "明日の会議について"
    llm_out = "About tomorrow's meeting."
    result = apply_translate_guard(raw, llm_out)
    assert result.flags == []


def test_translate_guard_rejects_too_much_japanese() -> None:
    raw = "明日の会議について"
    llm_out = "明日のmeetingについて話します"
    result = apply_translate_guard(raw, llm_out)
    assert result.flags == ["llm_rejected"]
    assert result.text == raw


def test_translate_guard_rejects_length_ratio_too_small() -> None:
    raw = "This is a reasonably long input sentence for translation testing purposes."
    llm_out = "Short."
    result = apply_translate_guard(raw, llm_out)
    assert result.flags == ["llm_rejected"]
