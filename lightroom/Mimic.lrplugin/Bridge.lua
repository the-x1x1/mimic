--[[
  Bridge client: discovery file → handshake → long-poll loop → results.

  Discovery: %LOCALAPPDATA%\Formicaria\Mimic\bridge\bridge.json (Windows) or
  ~/Library/Application Support/Formicaria/Mimic/bridge/bridge.json (macOS),
  overridable from the Plugin Manager. The file is written by the desktop app
  on every launch and holds the loopback base URL and a per-launch token.
]]

local LrHttp = import "LrHttp"
local LrTasks = import "LrTasks"
local LrPathUtils = import "LrPathUtils"
local LrFileUtils = import "LrFileUtils"
local LrPrefs = import "LrPrefs"
local LrApplication = import "LrApplication"
local LrMD5 = import "LrMD5"

local Json = require "Json"
local Logger = require "Logger"
local Version = require "Version"
local Capabilities = require "Capabilities"
local Commands = require "Commands"

local prefs = LrPrefs.prefsForPlugin()

local Bridge = {
  state = "idle", -- idle | discovering | handshaking | connected | disconnected | stopped
  lastError = nil,
  baseUrl = nil,
  sessionId = nil,
  pollIntervalMs = 1000,
  maxBatchSize = 25,
  connectedAt = nil,
  commandsHandled = 0,
  wantReconnect = false,
  _running = false,
  _token = nil,
}

local function defaultDiscoveryPath()
  if WIN_ENV then
    local base = os.getenv("LOCALAPPDATA")
    if base and base ~= "" then
      return LrPathUtils.child(LrPathUtils.child(LrPathUtils.child(LrPathUtils.child(base, "Formicaria"), "Mimic"), "bridge"), "bridge.json")
    end
  else
    local home = os.getenv("HOME")
    if home and home ~= "" then
      return home .. "/Library/Application Support/Formicaria/Mimic/bridge/bridge.json"
    end
  end
  return nil
end

function Bridge.discoveryPath()
  if prefs.discoveryPath and prefs.discoveryPath ~= "" then return prefs.discoveryPath end
  return defaultDiscoveryPath()
end

function Bridge.setDiscoveryPath(path)
  prefs.discoveryPath = path
end

local function readDiscovery()
  local path = Bridge.discoveryPath()
  if not path or not LrFileUtils.exists(path) then
    return nil, "Mimic is not running (no bridge file at " .. tostring(path) .. ")"
  end
  local ok, contents = pcall(LrFileUtils.readFile, path)
  if not ok or not contents or contents == "" then return nil, "cannot read bridge file" end
  local okJ, disc = pcall(Json.decode, contents)
  if not okJ or type(disc) ~= "table" then return nil, "bridge file is not valid JSON" end
  if disc.protocolVersion ~= Version.protocolVersion then
    return nil, string.format("Mimic speaks bridge protocol %s, plugin speaks %s — update the plugin from Mimic › Settings › Lightroom", tostring(disc.protocolVersion), tostring(Version.protocolVersion))
  end
  if type(disc.baseUrl) ~= "string" or not disc.baseUrl:match("^http://127%.0%.0%.1:%d+$") then
    return nil, "bridge file has an unexpected base URL (loopback only is allowed)"
  end
  if type(disc.token) ~= "string" or #disc.token < 32 then return nil, "bridge file has no token" end
  return disc
end

local function headers()
  return {
    { field = "Authorization", value = "Bearer " .. (Bridge._token or "") },
    { field = "Content-Type", value = "application/json" },
    { field = "Accept", value = "application/json" },
    { field = "User-Agent", value = "Mimic.lrplugin/" .. Version.version },
  }
end

-- LrHttp returns (body, headersTable); headersTable.status holds the HTTP code.
local function statusOf(hdrs)
  if type(hdrs) == "table" then
    if hdrs.status then return tonumber(hdrs.status) end
    if hdrs.error then return nil, hdrs.error end
  end
  return nil, "no response"
end

local function post(path, body, timeout)
  local url = Bridge.baseUrl .. path
  local payload = Json.encode(body)
  local resp, hdrs = LrHttp.post(url, payload, headers(), "POST", timeout or 15)
  local status, err = statusOf(hdrs)
  if not status then return nil, nil, tostring(err and (err.errorCode or err.name or err) or "network error") end
  local decoded = nil
  if resp and resp ~= "" then
    local ok, v = pcall(Json.decode, resp)
    if ok then decoded = v end
  end
  return status, decoded
end

local function get(path, timeout)
  local url = Bridge.baseUrl .. path
  local resp, hdrs = LrHttp.get(url, headers(), timeout or 15)
  local status, err = statusOf(hdrs)
  if not status then return nil, nil, tostring(err and (err.errorCode or err.name or err) or "network error") end
  local decoded = nil
  if resp and resp ~= "" and status ~= 204 then
    local ok, v = pcall(Json.decode, resp)
    if ok then decoded = v end
  end
  return status, decoded
end

function Bridge.catalogFingerprint(catalog)
  local path = catalog:getPath() or "unknown"
  return "md5:" .. LrMD5.digest(path)
end

function Bridge.postEvents(events)
  if Bridge.state ~= "connected" then return false end
  local status = post("/bridge/v1/events", { events = events }, 5)
  return status == 200
end

local function handshake(catalog)
  Bridge.state = "handshaking"
  local probe = Capabilities.probe(catalog)
  local catalogName = LrPathUtils.leafName(catalog:getPath() or "")
  local body = {
    protocolVersion = Version.protocolVersion,
    pluginVersion = Version.version,
    lightroomVersion = probe.lightroomVersion,
    sdkVersion = probe.sdkVersion,
    catalogFingerprint = Bridge.catalogFingerprint(catalog),
    catalogName = catalogName,
    capabilities = probe,
  }
  local status, resp, err = post("/bridge/v1/handshake", body, 20)
  if not status then return false, "cannot reach Mimic (" .. tostring(err) .. ")" end
  if status == 401 then return false, "Mimic rejected the plugin token — restart Mimic and use Mimic: Reconnect Now" end
  if status ~= 200 or type(resp) ~= "table" or resp.accepted ~= true then
    local reason = (type(resp) == "table" and resp.reason) or ("HTTP " .. tostring(status))
    return false, "handshake refused: " .. tostring(reason)
  end
  Bridge.sessionId = resp.sessionId
  Bridge.pollIntervalMs = tonumber(resp.pollIntervalMs) or 1000
  Bridge.maxBatchSize = tonumber(resp.maxBatchSize) or 25
  Bridge.connectedAt = os.time()
  Bridge.state = "connected"
  Bridge.lastError = nil
  Logger.info("connected to Mimic", { appVersion = resp.appVersion, sessionId = resp.sessionId })
  return true
end

local function runCommand(catalog, command)
  local handler = Commands[command.commandType]
  local result
  if not handler then
    result = { ok = false, error = { code = "unknown_command", message = "plugin does not implement " .. tostring(command.commandType) } }
  else
    local ok, out = pcall(handler, catalog, command.payload or {}, Bridge)
    if ok and type(out) == "table" and out.ok ~= nil then
      result = out
    elseif ok then
      result = { ok = true, result = out }
    else
      result = { ok = false, error = { code = "plugin_exception", message = tostring(out) } }
    end
  end
  result.commandId = command.commandId
  local status = post("/bridge/v1/commands/" .. command.commandId .. "/result", result, 30)
  Bridge.commandsHandled = Bridge.commandsHandled + 1
  if status ~= 200 then
    Logger.warn("result not accepted", { commandId = command.commandId, status = status })
  end
end

-- Main loop. Runs inside an async task; exits when Bridge.stop() is called.
function Bridge.run()
  if Bridge._running then return end
  Bridge._running = true
  local catalog = LrApplication.activeCatalog()
  local backoff = 2
  while Bridge._running do
    if Bridge.state ~= "connected" then
      Bridge.state = "discovering"
      local disc, err = readDiscovery()
      if not disc then
        Bridge.lastError = err
        Bridge.state = "disconnected"
        LrTasks.sleep(backoff)
        backoff = math.min(backoff * 2, 30)
      else
        Bridge.baseUrl = disc.baseUrl
        Bridge._token = disc.token
        local ok, herr = handshake(catalog)
        if not ok then
          Bridge.lastError = herr
          Bridge.state = "disconnected"
          Logger.warn("handshake failed", { error = herr })
          LrTasks.sleep(backoff)
          backoff = math.min(backoff * 2, 30)
        else
          backoff = 2
        end
      end
    else
      if Bridge.wantReconnect then
        Bridge.wantReconnect = false
        Bridge.state = "disconnected"
      else
        local waitMs = math.max(Bridge.pollIntervalMs, 250)
        local status, body, err = get("/bridge/v1/commands/next?waitMs=" .. tostring(math.min(waitMs * 5, 5000)), 15)
        if not status then
          Bridge.lastError = "lost connection to Mimic (" .. tostring(err) .. ")"
          Bridge.state = "disconnected"
        elseif status == 200 and type(body) == "table" and type(body.command) == "table" then
          runCommand(catalog, body.command)
        elseif status == 204 then
          LrTasks.sleep(waitMs / 1000)
        elseif status == 401 or status == 409 then
          Bridge.lastError = "Mimic asked the plugin to reconnect"
          Bridge.state = "disconnected"
        else
          LrTasks.sleep(waitMs / 1000)
        end
      end
    end
  end
  Bridge.state = "stopped"
end

function Bridge.start()
  LrTasks.startAsyncTask(function()
    local ok, err = pcall(Bridge.run)
    if not ok then
      Bridge.lastError = tostring(err)
      Bridge.state = "stopped"
      Bridge._running = false
      Logger.error("bridge loop crashed", { error = tostring(err) })
    end
  end, "Mimic bridge")
end

function Bridge.stop()
  Bridge._running = false
end

function Bridge.reconnect()
  Bridge.wantReconnect = true
  if not Bridge._running then Bridge.start() end
end

function Bridge.statusText()
  local lines = {
    "State: " .. tostring(Bridge.state),
    "Plugin version: " .. Version.version .. " (protocol " .. tostring(Version.protocolVersion) .. ")",
    "Bridge file: " .. tostring(Bridge.discoveryPath()),
  }
  if Bridge.baseUrl then lines[#lines + 1] = "Mimic at: " .. Bridge.baseUrl end
  if Bridge.sessionId then lines[#lines + 1] = "Session: " .. Bridge.sessionId end
  lines[#lines + 1] = "Commands handled: " .. tostring(Bridge.commandsHandled)
  if Bridge.lastError then lines[#lines + 1] = "Last error: " .. tostring(Bridge.lastError) end
  return table.concat(lines, "\n")
end

return Bridge
