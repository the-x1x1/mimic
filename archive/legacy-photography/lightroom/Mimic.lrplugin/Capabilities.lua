--[[
  Runtime capability probe. Nothing about develop settings is assumed: we ask
  the running Lightroom what it exposes and what a probe photo returns, and the
  desktop derives the capability matrix from that (mimic-core::capability).
]]

local LrApplication = import "LrApplication"
local LrTasks = import "LrTasks"
local Version = require "Version"
local Logger = require "Logger"

local Capabilities = {}

local function hasFunction(tbl, name)
  local ok, v = pcall(function() return tbl[name] end)
  return ok and type(v) == "function"
end

local function versionString()
  local ok, v = pcall(LrApplication.versionTable)
  if ok and type(v) == "table" then
    return string.format("%d.%d.%d", v.major or 0, v.minor or 0, v.revision or 0), v
  end
  local okS, s = pcall(LrApplication.versionString)
  return (okS and s) or "unknown", nil
end

-- Returns the probe table matching mimic-core::capability::CapabilityProbe.
function Capabilities.probe(catalog)
  local lrVersion, vt = versionString()
  local sdkVersion = vt and (tostring(vt.major) .. "." .. tostring(vt.minor)) or "unknown"
  local notes = {}
  local keys = {}
  local supports = {
    getDevelopSettings = false,
    applyDevelopPreset = false,
    addDevelopPresetForPlugin = hasFunction(LrApplication, "addDevelopPresetForPlugin"),
    createDevelopSnapshot = false,
    developController = false,
    catalogWriteAccess = false,
    lrHttp = pcall(import, "LrHttp"),
  }

  local okDC = pcall(import, "LrDevelopController")
  supports.developController = okDC and true or false

  local photo = nil
  local okTarget, target = pcall(function() return catalog:getTargetPhoto() end)
  if okTarget and target then photo = target end
  if not photo then
    local okAll, all = pcall(function() return catalog:getTargetPhotos() end)
    if okAll and all and #all > 0 then photo = all[1] end
  end

  if photo then
    supports.getDevelopSettings = hasFunction(photo, "getDevelopSettings")
    supports.applyDevelopPreset = hasFunction(photo, "applyDevelopPreset")
    supports.createDevelopSnapshot = hasFunction(photo, "createDevelopSnapshot")
    if supports.getDevelopSettings then
      local okDS, settings = pcall(function() return photo:getDevelopSettings() end)
      if okDS and type(settings) == "table" then
        for k in pairs(settings) do keys[#keys + 1] = tostring(k) end
        table.sort(keys)
        notes[#notes + 1] = "probe photo: " .. tostring(photo:getFormattedMetadata("fileName") or "?")
      else
        notes[#notes + 1] = "getDevelopSettings failed on probe photo"
      end
    end
  else
    notes[#notes + 1] = "no photo selected at probe time; select a photo and use Mimic: Reconnect Now"
  end

  -- A no-op write-access transaction proves we can write to this catalog.
  local okWrite, errWrite = pcall(function()
    local result = catalog:withWriteAccessDo("Mimic capability probe", function() end, { timeout = 5 })
    if result == "executed" then supports.catalogWriteAccess = true end
  end)
  if not okWrite then
    notes[#notes + 1] = "write access probe failed: " .. tostring(errWrite)
  end

  Logger.info("capability probe", { lightroomVersion = lrVersion, keys = #keys, supports = supports })
  return {
    lightroomVersion = lrVersion,
    sdkVersion = sdkVersion,
    pluginVersion = Version.version,
    developSettingKeys = keys,
    supports = supports,
    notes = notes,
  }
end

-- Small helper so the probe can be refreshed on demand from a background task.
function Capabilities.probeAsync(catalog, callback)
  LrTasks.startAsyncTask(function()
    local ok, probe = pcall(Capabilities.probe, catalog)
    if ok then callback(probe) else callback(nil, probe) end
  end)
end

return Capabilities
