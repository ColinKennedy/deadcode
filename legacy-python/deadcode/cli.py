from typing import List, Optional
import sys

from deadcode import __version__
from deadcode.actions.find_python_filenames import find_python_filenames
from deadcode.actions.find_unused_names import find_unused_names
from deadcode.actions.fix_or_show_unused_code import fix_or_show_unused_code
from deadcode.actions.parse_arguments import parse_arguments
from deadcode.actions.get_unused_names_error_message import (
    get_unused_names_error_message,
)


def main(
    command_line_args: Optional[List[str]] = None,
) -> Optional[str]:
    if command_line_args and '--version' in command_line_args or '--version' in sys.argv:
        return __version__

    args = parse_arguments(command_line_args)

    filenames = find_python_filenames(args=args)

    # TODO: rename unused_names to unused_code_items
    unused_names = find_unused_names(filenames=filenames, args=args)

    file_diff = None
    if (args.fix or args.dry) and unused_names:
        file_diff = fix_or_show_unused_code(unused_names, args=args)

    if (error_message := get_unused_names_error_message(unused_names, args=args)) is not None:
        return error_message + ('\n\n' + file_diff if file_diff else '')

    if not args.count and not args.quiet:
        try:
            print('\033[1mWell done!\033[0m ✨ 🚀 ✨')
        except UnicodeEncodeError:
            # Some terminals (e.g. Windows' default cp1252 console codepage) cannot
            # encode emoji. Fall back to a plain message rather than crashing.
            print('\033[1mWell done!\033[0m')
    return None


def print_main() -> None:
    is_version_request = '--version' in sys.argv

    result = main()
    if result:
        print(result)

    if not is_version_request and result is not None:
        sys.exit(1)


if __name__ == '__main__':
    print_main()
