from __future__ import annotations

from voice_server.text_rules import detect_repetition, strip_fillers


def test_strip_fillers_removes_simple_filler() -> None:
    assert strip_fillers("えっと明日会議です") == "明日会議です"


def test_strip_fillers_removes_ano_filler_but_keeps_ano_hito() -> None:
    assert strip_fillers("あの明日の件ですが") == "明日の件ですが"
    assert strip_fillers("あの人が来ました") == "あの人が来ました"


def test_strip_fillers_removes_multiple_fillers() -> None:
    assert strip_fillers("まあなんか今日は忙しいです") == "今日は忙しいです"


def test_detect_repetition_true_for_triple_repeat() -> None:
    assert detect_repetition("ありがとうありがとうありがとう") is True


def test_detect_repetition_false_for_double_repeat() -> None:
    assert detect_repetition("ありがとうありがとう") is False


def test_detect_repetition_false_for_normal_text() -> None:
    assert detect_repetition("明日の会議の資料を確認しておきます") is False


def test_detect_repetition_requires_min_repeats_of_at_least_three() -> None:
    import pytest

    with pytest.raises(ValueError):
        detect_repetition("abc", min_repeats=2)
