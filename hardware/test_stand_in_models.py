from collections import Counter

import pytest
from ee_tooling.step_model import Solid

from stand_in_models import MODELS


def closed(solid: Solid) -> bool:
    """Every edge in exactly one face each way: a closed, consistently wound shell."""
    e = Counter(
        (a, b) for face in solid.faces for a, b in zip(face, face[1:] + face[:1], strict=True)
    )
    return all(n == 1 and e[(b, a)] == 1 for (a, b), n in e.items())


@pytest.mark.parametrize("path", list(MODELS))
def test_models_closed(path: str) -> None:
    assert all(closed(s) for s in MODELS[path]())
