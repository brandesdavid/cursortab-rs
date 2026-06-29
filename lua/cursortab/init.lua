local M = {}

local defaults = {
  -- Highlight for inline ghost text
  suggestion_hl = { fg = "#808080", italic = true },
  -- Highlight for next-cursor « hint
  next_hint_hl = { fg = "#d0a060" },
  -- Key to accept suggestion
  accept_key = "<Tab>",
}

local chan
local ns_id

local function bin_path()
  -- Prefer binary shipped next to the plugin (built via `build.sh` or lazy `build`)
  local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h")
  local bundled = plugin_dir .. "/bin/cursortab"
  if vim.fn.executable(bundled) == 1 then
    return bundled
  end
  -- Fall back to system PATH (e.g. installed via Nix)
  if vim.fn.executable("cursortab") == 1 then
    return "cursortab"
  end
  return nil
end

local function ensure_job(bin)
  if chan then return chan end
  chan = vim.fn.jobstart({ bin }, { rpc = true })
  if chan <= 0 then
    chan = nil
    vim.notify("cursortab: failed to start binary at " .. bin, vim.log.levels.ERROR)
    return nil
  end
  return chan
end

function M.setup(opts)
  opts = vim.tbl_deep_extend("force", defaults, opts or {})

  local bin = bin_path()
  if not bin then
    vim.notify(
      "cursortab: binary not found. Run the build step or install via Nix.\n"
        .. "  build: cd <plugin-dir> && cargo build --release && cp target/release/cursortab bin/",
      vim.log.levels.WARN
    )
    return
  end

  ns_id = vim.api.nvim_create_namespace("cursortab")

  vim.api.nvim_set_hl(0, "CursorTabSuggestion", opts.suggestion_hl)
  vim.api.nvim_set_hl(0, "CursorTabNextHint", opts.next_hint_hl)

  vim.api.nvim_create_autocmd({ "TextChanged", "TextChangedI" }, {
    callback = function()
      local c = ensure_job(bin)
      if not c then return end
      local ok, err = pcall(vim.fn.rpcrequest, c, "cursortab_sync", ns_id)
      if not ok then
        chan = nil
      end
    end,
  })

  vim.keymap.set("i", opts.accept_key, function()
    local c = ensure_job(bin)
    if not c then return end
    local ok, err = pcall(vim.fn.rpcrequest, c, "cursortab_tab_key", ns_id)
    if not ok then
      chan = nil
    end
  end, { noremap = true, silent = true })
end

return M
