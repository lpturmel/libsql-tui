# libsql-tui

A query runner TUI for Turso hosted [LibSQL](https://github.com/tursodatabase/libsql) databases.

## Features

- Query runner
- Tabbed query editor (with some vim keybinds)
- Query result viewer

## Usage

Since the project is still in development, it only works by compiling it.

1. Clone the repository

```
git clone https://github.com/lpturmel/libsql-tui
```

2. Build the project

```
cargo build --release
```

3. Add the binary to your path or run it directly

```
./target/release/libsqltui
```


## Configuration

To use the app, you need to have the [turso cli](https://docs.turso.tech/cli/installation) installed and configured on your system.

1. Login to your account


 ```
 turso auth login
 ```

2. Populate the config with your databases

```
turso db list
```

3. Connect to a database

Generate a token for the database you want to connect to

```
turso db shell DB_NAME
```

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
