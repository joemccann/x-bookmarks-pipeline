"""
Shared Rich console for pipeline output.
"""
from rich.console import Console
from rich.theme import Theme

theme = Theme({
    "info": "cyan",
    "success": "bold green",
    "warning": "bold yellow",
    "error": "bold red",
    "cached": "dim yellow",
    "skip": "dim",
    "step": "bold cyan",
    "label": "bold white",
    "dim": "dim",
    "muted": "dim italic",
    "author": "bold magenta",
    "ticker": "bold cyan",
    "tag": "bold blue",
})

console = Console(theme=theme, highlight=False)
