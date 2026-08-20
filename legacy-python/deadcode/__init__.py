_DISTRIBUTION_NAME = 'lapsed'  # PyPI project name; differs from this importable package name ("deadcode").

try:
    import importlib.metadata

    __version__ = importlib.metadata.version(_DISTRIBUTION_NAME)
except ImportError:
    import importlib_metadata

    __version__ = importlib_metadata.version(_DISTRIBUTION_NAME)
