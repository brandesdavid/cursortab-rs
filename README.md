# cursortab.nvim (Rust)

Cursor AI tab completions for Neovim - a Rust rewrite of [cursortab.nvim](https://github.com/reachingforthejack/cursortab.nvim).

**Improvements over the original Go version:**
- Inline ghost text (`virt_text_pos = "inline"`) - suggestion scrolls with your buffer instead of sticking in the middle
- `«` hint after accepting a completion, pointing to the predicted next cursor position
- No `sqlite3` binary dependency - reads your Cursor auth token directly via `rusqlite`
- Linux + macOS support
- Request cancellation via `CancellationToken` - no stale completions

## Requirements

- [Cursor](https://www.cursor.com/) installed and signed in (auth token is read from its local SQLite DB)
- Rust toolchain (`cargo`) for the build step

## Installation

### lazy.nvim

```lua
return {
  "brandesdavid/cursortab-rs",
  build = "./build.sh",
  opts = {},
}
```

That's it - `build.sh` compiles the Rust binary on first install and on `:Lazy build cursortab-rs`.

### Custom options

```lua
return {
  "brandesdavid/cursortab-rs",
  build = "./build.sh",
  opts = {
    accept_key   = "<Tab>",                    -- key to accept suggestion
    suggestion_hl = { fg = "#808080", italic = true },  -- ghost text color
    next_hint_hl  = { fg = "#d0a060" },        -- « hint color
  },
}
```

### NixOS / home-manager

If you manage Neovim via Nix you can use the provided derivation instead of the build script:

```nix
# In your neovim module:
programs.neovim.extraPackages = [
  (pkgs.callPackage ./cursortab-rs-binary.nix { cursortabRsSrc = inputs.cursortab-rs; })
];
```

## How it works

On each `TextChanged` / `TextChangedI` event, the plugin spawns the `cursortab` binary as a Neovim RPC server (msgpack over stdin/stdout). The binary calls the Cursor AI API (`api2.cursor.sh`) using the same Connect RPC protocol Cursor itself uses, then renders the suggestion as inline virtual text. Pressing `<Tab>` accepts the suggestion and shows a `«` hint at the predicted next edit location.
