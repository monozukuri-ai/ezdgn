from __future__ import annotations

import subprocess
import sys

import ezdgn


def test_python_and_rust_versions_match() -> None:
    assert ezdgn.__version__ == ezdgn._core.core_version()
    assert all(hasattr(ezdgn, name) for name in ezdgn.__all__)
    assert {
        "AttributeLinkage",
        "Cell",
        "TextNode",
        "ComplexChain",
        "ComplexShape",
        "Curve",
        "BSplineCurve",
        "BSplineSurface",
        "DgnAttributes",
        "Modelspace",
        "V7Document",
        "V7WriteEntity",
        "V8CfbEntry",
        "V8ContainerInfo",
        "inspect_v8_container",
        "new",
    }.issubset(ezdgn.__all__)


def test_cli_version() -> None:
    result = subprocess.run(
        [sys.executable, "-m", "ezdgn", "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    assert result.stdout.strip() == f"ezdgn {ezdgn.__version__}"
