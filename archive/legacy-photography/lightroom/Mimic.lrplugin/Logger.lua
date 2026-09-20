-- Structured-ish logging to Lightroom's plugin log (Documents/LrClassicLogs/Mimic.log).
-- Never logs tokens or pixel data.

local LrLogger = import "LrLogger"
local logger = LrLogger("Mimic")
logger:enable("logfile")

local Logger = {}

local function fmt(level, msg, data)
  local line = string.format("[%s] %s", level, tostring(msg))
  if data ~= nil then
    local ok, Json = pcall(require, "Json")
    if ok then
      local okEnc, encoded = pcall(Json.encode, data)
      if okEnc then line = line .. " " .. encoded end
    end
  end
  return line
end

function Logger.info(msg, data) logger:info(fmt("info", msg, data)) end
function Logger.warn(msg, data) logger:warn(fmt("warn", msg, data)) end
function Logger.error(msg, data) logger:error(fmt("error", msg, data)) end
function Logger.trace(msg, data) logger:trace(fmt("trace", msg, data)) end

return Logger
