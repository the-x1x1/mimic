local LrDialogs = import "LrDialogs"
local LrTasks = import "LrTasks"
local Bridge = require "Bridge"

LrTasks.startAsyncTask(function()
  LrDialogs.message("Mimic connection", Bridge.statusText(), "info")
end)
