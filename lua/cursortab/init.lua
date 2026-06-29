local M = {}

local defaults = {
  suggestion_hl = { fg = "#808080", italic = true },
  next_hint_hl  = { fg = "#d0a060" },
  accept_key    = "<Tab>",
}

local chan
local ns_id
local has_suggestion = false  -- true while ghost text is visible

local function bin_path()
  local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h")
  local bundled = plugin_dir .. "/bin/cursortab"
  if vim.fn.executable(bundled) == 1 then
    return bundled
  end
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
  vim.api.nvim_set_hl(0, "CursorTabNextHint",   opts.next_hint_hl)

  vim.api.nvim_create_autocmd({ "TextChanged", "TextChangedI" }, {
    callback = function()
      local c = ensure_job(bin)
      if not c then return end
      -- Clear suggestion flag on every keystroke; binary will set a new one
      has_suggestion = false
      local ok = pcall(vim.fn.rpcrequest, c, "cursortab_sync", ns_id)
      if not ok then
        chan = nil
        has_suggestion = false
      end
    end,
  })

  -- After each sync the binary places extmarks; detect them to set has_suggestion
  vim.api.nvim_create_autocmd("CursorMovedI", {
    callback = function()
      if not ns_id then return end
      local buf = vim.api.nvim_get_current_buf()
      local marks = vim.api.nvim_buf_get_extmarks(buf, ns_id, 0, -1, {})
      has_suggestion = #marks > 0
    end,
  })

  vim.keymap.set("i", opts.accept_key, function()
    if not has_suggestion then
      -- No suggestion visible: insert a literal tab
      return "\t"
    end
    has_suggestion = false
    local c = ensure_job(bin)
    if not c then return end
    local ok = pcall(vim.fn.rpcrequest, c, "cursortab_tab_key", ns_id)
    if not ok then
      chan = nil
    end
  end, { noremap = true, silent = true, expr = true })
end

return M
