#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path

from smoke_test.harness import SmokeHarness
from smoke_test import (
    test_agent_turn,
    test_extensions,
    test_integration,
    test_model_roles,
    test_model_settings,
)


def main() -> None:
    root = Path(__file__).resolve().parent.parent

    with SmokeHarness(root) as harness:
        for module in (
            test_extensions,
            test_model_roles,
            test_model_settings,
            test_agent_turn,
            test_integration,
        ):
            print()
            print(f"═══ {module.__name__.split('.')[-1]} ═══")
            module.run(harness)

    print()
    print("All smoke tests complete.")


if __name__ == "__main__":
    main()
