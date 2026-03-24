#!/usr/bin/env python3

from __future__ import annotations

import sys
from pathlib import Path

from smoke_test.harness import SmokeHarness
from smoke_test import (
    test_agent_turn,
    test_extensions,
    test_google_provider,
    test_model_roles,
    test_model_settings,
    test_openrouter_provider,
)

ALL_MODULES = (
    test_extensions,
    test_model_roles,
    test_model_settings,
    test_agent_turn,
    test_google_provider,
    test_openrouter_provider,
)


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    filter_arg = sys.argv[1] if len(sys.argv) > 1 else None

    modules = ALL_MODULES
    if filter_arg:
        modules = tuple(m for m in ALL_MODULES if filter_arg in m.__name__)
        if not modules:
            print(f"No smoke test matching '{filter_arg}'. Available:")
            for m in ALL_MODULES:
                print(f"  {m.__name__.split('.')[-1]}")
            sys.exit(1)

    with SmokeHarness(root) as harness:
        for module in modules:
            print()
            print(f"═══ {module.__name__.split('.')[-1]} ═══")
            module.run(harness)

    print()
    print("All smoke tests complete.")


if __name__ == "__main__":
    main()
