from __future__ import annotations

import pytest

from voice_server.dictionary import (
    Dictionary,
    DictionaryStore,
    DictionaryTerm,
    apply_fixed_replacements,
    build_asr_context,
    select_glossary_terms,
)

TERMS = [
    DictionaryTerm(surface="Kubernetes", aliases=["クバネティス", "クーベネティス"], replace=True),
    DictionaryTerm(surface="Meta", aliases=["メタ"], replace=False),
]


def test_apply_fixed_replacements_replaces_alias_with_surface() -> None:
    text = "クバネティスの勉強をしています"
    assert apply_fixed_replacements(text, TERMS) == "Kubernetesの勉強をしています"


def test_apply_fixed_replacements_ignores_non_replace_terms() -> None:
    text = "メタの話をしました"
    assert apply_fixed_replacements(text, TERMS) == "メタの話をしました"


def test_apply_fixed_replacements_prefers_longer_alias_first() -> None:
    terms = [
        DictionaryTerm(surface="AB", aliases=["A"], replace=True),
        DictionaryTerm(surface="XY", aliases=["AB"], replace=True),
    ]
    # "AB" (長い) を先に XY へ置換すべきで、"A" による部分置換で壊れてはいけない
    assert apply_fixed_replacements("AB", terms) == "XY"


def test_build_asr_context_limits_to_max_terms() -> None:
    many_terms = [DictionaryTerm(surface=f"term{i}") for i in range(150)]
    context = build_asr_context(many_terms, max_terms=100)
    assert len(context.split(" ")) == 100
    assert context.split(" ")[0] == "term0"


def test_select_glossary_terms_matches_surface_occurrence() -> None:
    selected = select_glossary_terms("Kubernetesを使っています", TERMS)
    assert TERMS[0] in selected


def test_select_glossary_terms_matches_katakana_alias() -> None:
    selected = select_glossary_terms("クーベネティスの勉強", TERMS)
    assert TERMS[0] in selected


def test_select_glossary_terms_limits_to_max_terms() -> None:
    many_terms = [DictionaryTerm(surface=f"用語{i}") for i in range(40)]
    text = "".join(f"用語{i}" for i in range(40))
    selected = select_glossary_terms(text, many_terms, max_terms=30)
    assert len(selected) == 30


def test_select_glossary_terms_empty_text_returns_empty() -> None:
    assert select_glossary_terms("", TERMS) == []


def test_dictionary_store_save_and_reload(tmp_path) -> None:
    path = tmp_path / "dictionary.yaml"
    store = DictionaryStore(path)
    assert store.get().terms == []

    dictionary = Dictionary(terms=[DictionaryTerm(surface="Rust", aliases=["ラスト"], replace=True)])
    store.save(dictionary)

    reloaded = DictionaryStore(path)
    assert reloaded.get().terms[0].surface == "Rust"


def test_dictionary_store_save_rejects_invalid_payload(tmp_path) -> None:
    from jsonschema import ValidationError

    path = tmp_path / "dictionary.yaml"
    store = DictionaryStore(path)
    invalid = Dictionary(terms=[DictionaryTerm(surface="")])  # protocol schema: minLength 1
    with pytest.raises(ValidationError):
        store.save(invalid)


def test_dictionary_store_keeps_previous_content_on_corrupt_yaml(tmp_path) -> None:
    import os
    import time

    path = tmp_path / "dictionary.yaml"
    store = DictionaryStore(path)
    store.save(Dictionary(terms=[DictionaryTerm(surface="Rust", aliases=["ラスト"], replace=True)]))

    # 外部プロセスなどによってファイルが壊れた YAML に書き換えられたケースを模す。
    # mtime がポーリング対象になるよう、確実に将来の時刻へ変更する。
    path.write_text("terms: [broken: : :", encoding="utf-8")
    future = time.time() + 5
    os.utime(path, (future, future))

    result = store.get()
    assert result.terms[0].surface == "Rust"


def test_dictionary_store_save_is_atomic_and_leaves_no_tmp_file(tmp_path) -> None:
    path = tmp_path / "dictionary.yaml"
    store = DictionaryStore(path)
    store.save(Dictionary(terms=[DictionaryTerm(surface="Go")]))

    leftover = [p for p in tmp_path.iterdir() if p.name != "dictionary.yaml"]
    assert leftover == []
    assert path.read_text(encoding="utf-8")  # 内容が書き込まれている
