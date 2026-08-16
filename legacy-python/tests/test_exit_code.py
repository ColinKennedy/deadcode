import sys
from unittest.mock import patch

import pytest

from deadcode.cli import main, print_main
from deadcode.utils.base_test_case import BaseTestCase


class TestExitCode(BaseTestCase):
    def test_exits_with_status_1_when_dead_code_is_found(self):
        self.files = {
            'foo.py': b"""
                unused_variable = "Hello"
                """
        }

        with patch.object(sys, 'argv', ['deadcode', 'foo.py', '--no-color']), pytest.raises(SystemExit) as ctx:
            print_main()

        self.assertEqual(ctx.value.code, 1)

    def test_exits_with_status_0_when_no_dead_code_is_found(self):
        self.files = {
            'foo.py': b"""
                used_variable = "Hello"
                print(used_variable)
                """
        }

        with patch.object(sys, 'argv', ['deadcode', 'foo.py', '--no-color']):
            try:
                print_main()
            except SystemExit as exc:
                self.fail(f'print_main() unexpectedly called sys.exit({exc.code})')

    def test_exits_with_status_1_when_quiet_and_dead_code_is_found(self):
        # Regression test: --quiet makes main() return an empty string (falsy) when dead
        # code is found. print_main() must not mistake that falsy value for "nothing
        # found" -- the process still has to fail.
        self.files = {
            'foo.py': b"""
                unused_variable = "Hello"
                """
        }

        with (
            patch.object(sys, 'argv', ['deadcode', 'foo.py', '--no-color', '--quiet']),
            pytest.raises(SystemExit) as ctx,
        ):
            print_main()

        self.assertEqual(ctx.value.code, 1)

    def test_does_not_exit_with_error_status_for_version_flag(self):
        with patch.object(sys, 'argv', ['deadcode', '--version']):
            try:
                print_main()
            except SystemExit as exc:
                self.fail(f'print_main() unexpectedly called sys.exit({exc.code})')

    def test_success_message_falls_back_when_terminal_cannot_encode_emoji(self):
        # Regression test: some terminals (e.g. Windows' default cp1252 console codepage)
        # raise UnicodeEncodeError when printing emoji. main() must not crash -- it should
        # fall back to a plain message instead.
        self.files = {
            'foo.py': b"""
                used_variable = "Hello"
                print(used_variable)
                """
        }

        original_print = print

        def fake_print(message: str = '', *args: object, **kwargs: object) -> None:
            if '✨' in message:
                reason = 'character maps to <undefined>'
                encoding = 'cp1252'
                raise UnicodeEncodeError(encoding, message, 0, 1, reason)
            original_print(message, *args, **kwargs)

        with patch('deadcode.cli.print', side_effect=fake_print):
            result = main(['foo.py', '--no-color'])

        self.assertEqual(result, None)
