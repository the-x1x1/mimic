-- Plugin Manager panel: shows status and lets the user override the bridge file path.
local LrView = import "LrView"
local LrDialogs = import "LrDialogs"
local LrBinding = import "LrBinding"
local Bridge = require "Bridge"
local Version = require "Version"

return {
  sectionsForTopOfDialog = function(f, propertyTable)
    propertyTable.discoveryPath = Bridge.discoveryPath() or ""
    propertyTable.status = Bridge.statusText()
    return {
      {
        title = "Mimic",
        bind_to_object = propertyTable,
        f:row { f:static_text { title = "Plugin version " .. Version.version .. " — the desktop app owns the connection; this plugin only polls it over 127.0.0.1." } },
        f:row {
          f:static_text { title = "Bridge file:", width = LrView.share("label") },
          f:edit_field { value = LrView.bind("discoveryPath"), width_in_chars = 60 },
          f:push_button {
            title = "Choose…",
            action = function()
              local files = LrDialogs.runOpenPanel { title = "Select bridge.json written by Mimic", canChooseFiles = true, canChooseDirectories = false, allowsMultipleSelection = false, fileTypes = "json" }
              if files and files[1] then propertyTable.discoveryPath = files[1] end
            end,
          },
        },
        f:row {
          f:push_button {
            title = "Save and reconnect",
            action = function()
              Bridge.setDiscoveryPath(propertyTable.discoveryPath)
              Bridge.reconnect()
            end,
          },
          f:push_button {
            title = "Refresh status",
            action = function() propertyTable.status = Bridge.statusText() end,
          },
        },
        f:row { f:static_text { title = LrView.bind("status"), height_in_lines = 7, width_in_chars = 80 } },
      },
    }
  end,
}
