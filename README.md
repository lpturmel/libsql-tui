> [!WARNING]  
> This project is still in development and is not yet ready for use.

# libsql-tui

A query runner TUI for Turso hosted [LibSQL](https://github.com/tursodatabase/libsql) databases.

## Usage

Since the project is still in development, it only works by compiling it.

The project will also migrate to a CLI interface in the future to specify which
database in your Turso account to connect to.

`cargo run`


## Key Bindings

| Key | Action |
| --- | --- |
| `i` | Enter insert mode |
| `a` | Move cursor to the end of the char and enter insert mode |
| `q` | Quit |
| `0` | Move cursor to the beginning of the input |
| `$` | Move cursor to the end of the input |
| `c` | Clear the results |
| `h` | Move cursor to the left |
| `l` | Move cursor to the right |
| `x` | Delete character under cursor |
| `D` | Clear the query |
| `Ctrl + r` | Submit the query |
| `Ctrl + n` | New query tab |
| `Ctrl + w` | Delete current query tab |
| `H` | Previous query tab |
| `L` | Next query tab |
| `w` | Move to the next word |
| `b` | Move to the previous word |

## Screenshot

![Screenshot](screenshot.jpg?)
