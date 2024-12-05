# libsql-tui

A query runner TUI for Turso hosted [LibSQL](https://github.com/tursodatabase/libsql) databases.

## Usage

Since the project is still in development, it only works by compiling it.

`cargo run`


## Configuration

To use the app, you need to have the [turso cli](https://docs.turso.tech/cli/installation) installed and configured on your system.

Run `turso login` to login to your account.

Then, to populate the config with your databases, run `turso db list` and connect to a database `turso db shell DB_NAME` this will create a token which the TUI can use to connect.

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
| `Ctrl` + `r` | Submit the query |
| `Ctrl` + `n` | New query tab |
| `Ctrl` + `w` | Delete current query tab |
| `Ctrl` + `t` | List database tables |
| `H` | Previous query tab |
| `L` | Next query tab |
| `w` | Move to the next word |
| `b` | Move to the previous word |

## Screenshot

![Screenshot](screenshot.jpg?)
