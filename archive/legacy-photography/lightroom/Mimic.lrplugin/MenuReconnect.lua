local LrDialogs = import "LrDialogs"
local LrTasks = import "LrTasks"
local Bridge = require "Bridge"

LrTasks.startAsyncTask(function()
  Bridge.reconnect()
  LrTasks.sleep(3)
  LrDialogs.showBezel(Bridge.state == "connected" and "Mimic: connected" or ("Mimic: " .. tostring(Bridge.lastError or Bridge.state)), 3)
end)
