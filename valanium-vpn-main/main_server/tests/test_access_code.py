import re

import pytest

from app.services import access_code


def test_generated_code_looks_like_a_code():
    code = access_code.generate()
    assert re.fullmatch(r"[0-9A-HJ-KM-NP-TV-Z]{5}(-[0-9A-HJ-KM-NP-TV-Z]{5}){3}", code), code
    assert access_code.normalize(code) == code.replace("-", "")


def test_generated_codes_differ():
    codes = {access_code.generate() for _ in range(200)}
    assert len(codes) == 200


@pytest.mark.parametrize(
    "written",
    [
        "{lower}",
        "{spaced}",
        "{nodash}",
        "  {code}  ",
    ],
)
def test_human_writing_is_accepted(written):
    code = access_code.generate()
    variants = {
        "lower": code.lower(),
        "spaced": code.replace("-", " "),
        "nodash": code.replace("-", ""),
        "code": code,
    }
    assert access_code.normalize(written.format(**variants)) == code.replace("-", "")


def test_confusable_letters_read_as_digits():
    """Человек, написавший O вместо 0, не должен получить «неверный код»."""
    canonical = access_code.normalize(access_code.generate())
    with_letters = canonical.replace("0", "O").replace("1", "I")
    assert access_code.normalize(with_letters) == canonical


def test_typo_is_rejected_by_checksum():
    canonical = access_code.normalize(access_code.generate())
    # Меняем один значащий знак на соседний по алфавиту.
    idx = 0
    pos = access_code.ALPHABET.index(canonical[idx])
    broken = access_code.ALPHABET[(pos + 1) % 32] + canonical[1:]
    assert access_code.normalize(broken) is None


def test_wrong_length_and_alien_characters_are_rejected():
    assert access_code.normalize("") is None
    assert access_code.normalize("SHORT") is None
    assert access_code.normalize(access_code.generate() + "A") is None
    assert access_code.normalize("!!!!!-!!!!!-!!!!!-!!!!!") is None


def test_hash_is_deterministic_and_peppered():
    code = access_code.generate()
    pepper_a, pepper_b = b"a" * 32, b"b" * 32
    assert access_code.hash_code(code, pepper_a) == access_code.hash_code(code, pepper_a)
    # Разные записи одного кода дают один хеш — иначе вход зависел бы от того,
    # как человек его переписал.
    assert access_code.hash_code(code.lower(), pepper_a) == access_code.hash_code(code, pepper_a)
    assert access_code.hash_code(code, pepper_a) != access_code.hash_code(code, pepper_b)


def test_hash_refuses_garbage():
    with pytest.raises(ValueError):
        access_code.hash_code("NOT-A-CODE", b"x" * 32)


def test_pepper_file_is_created_once(tmp_path):
    path = tmp_path / "sub" / "pepper"
    first = access_code.load_or_create_pepper(path)
    second = access_code.load_or_create_pepper(path)
    assert first == second
    assert len(first) == 32
