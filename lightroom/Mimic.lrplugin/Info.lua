--[[
  Mimic.lrplugin — Lightroom Classic plugin for Mimic (Formicaria).

  The plugin is a thin client: it polls the Mimic desktop app over loopback
  HTTP, executes a small, fixed set of commands against the catalog through
  official SDK calls, and reports results. It never touches the .lrcat file,
  never writes XMP, and never sends image pixels.
]]

return {
  LrSdkVersion = 13.0,
  LrSdkMinimumVersion = 6.0,
  LrToolkitIdentifier = "com.formicaria.mimic",
  LrPluginName = "Mimic",
  LrPluginInfoUrl = "https://github.com/the-x1x1/mimic",

  LrInitPlugin = "Init.lua",
  LrShutdownPlugin = "Shutdown.lua",
  LrPluginInfoProvider = "PluginInfoProvider.lua",

  LrLibraryMenuItems = {
    { title = "Mimic: Connection Status…", file = "MenuStatus.lua" },
    { title = "Mimic: Reconnect Now", file = "MenuReconnect.lua" },
  },

  -- Kept in lock-step with the desktop app by scripts/sync-version.mjs.
  VERSION = { major = 0, minor = 2, revision = 0, build = 1, display = "0.2.0-alpha.1" },
}
