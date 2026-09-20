--[[
  Develop-settings read/apply through official SDK calls only.

  Apply path (spec §10.4): snapshot → plugin-owned develop preset → apply inside
  withWriteAccessDo → read back → return before/readBack per photo. Success is
  decided by the desktop after comparing intended vs. read-back values; the
  plugin never claims success just because an SDK call did not throw.
]]

local LrApplication = import "LrApplication"
local Logger = require "Logger"
local Json = require "Json"

local Develop = {}

function Develop.getSettings(photo)
  local ok, settings = pcall(function() return photo:getDevelopSettings() end)
  if not ok then return nil, tostring(settings) end
  return settings
end

-- Lightroom preset settings must be plain Lua tables of develop keys.
local function sanitizeSettings(settings)
  local out = {}
  local count = 0
  for k, v in pairs(settings or {}) do
    if type(k) == "string" and v ~= Json.null then
      out[k] = v
      count = count + 1
    end
  end
  return out, count
end

local function presetName(predictionId)
  return "Mimic " .. tostring(predictionId or "apply")
end

-- Applies `settings` to `photo` with an optional before-snapshot. Must be called
-- inside catalog:withWriteAccessDo. Returns a result item table.
local function applyOne(catalog, photo, item, createSnapshot, readBack)
  local result = { photoId = item.photoId, predictionId = item.predictionId, status = "failed" }
  local settings, count = sanitizeSettings(item.settings)
  if count == 0 then
    result.status = "skipped"
    result.error = { code = "empty_settings", message = "no writable settings for this photo" }
    return result
  end

  local before = Develop.getSettings(photo)
  if before then result.before = before end

  if createSnapshot and item.snapshotName and type(photo.createDevelopSnapshot) == "function" then
    local okSnap, errSnap = pcall(function() photo:createDevelopSnapshot(item.snapshotName, true) end)
    if okSnap then
      result.snapshotName = item.snapshotName
    else
      result.error = { code = "snapshot_failed", message = tostring(errSnap) }
      return result -- never apply without the requested safety net
    end
  end

  local okPreset, preset = pcall(function()
    return LrApplication.addDevelopPresetForPlugin(_PLUGIN, presetName(item.predictionId), settings)
  end)
  if not okPreset or not preset then
    result.error = { code = "preset_failed", message = tostring(preset) }
    return result
  end
  local okApply, errApply = pcall(function() photo:applyDevelopPreset(preset, _PLUGIN) end)
  if not okApply then
    result.error = { code = "apply_failed", message = tostring(errApply) }
    return result
  end
  if readBack then
    local after, err = Develop.getSettings(photo)
    if after then
      result.readBack = after
      result.status = "applied"
    else
      result.error = { code = "readback_failed", message = tostring(err) }
    end
  else
    result.status = "applied"
  end
  return result
end

-- Batch apply with bounded size, per-photo results and cancellation between photos.
function Develop.applyBatch(catalog, payload, bridge, findPhoto)
  local items = payload.items or {}
  local maxBatch = (bridge and bridge.maxBatchSize) or 25
  if #items > maxBatch then
    return { ok = false, error = { code = "batch_too_large", message = string.format("batch of %d exceeds max %d", #items, maxBatch) } }
  end
  local createSnapshot = payload.createSnapshot ~= false
  local readBack = payload.readBack ~= false
  local results = {}
  local canceled = false

  for _, item in ipairs(items) do
    if bridge and bridge.cancelRequested then
      canceled = true
      break
    end
    local photo = findPhoto(item.photoId)
    if not photo then
      results[#results + 1] = { photoId = item.photoId, predictionId = item.predictionId, status = "failed", error = { code = "photo_not_found", message = "No photo with local identifier " .. tostring(item.photoId) .. " in this catalog" } }
    else
      local outcome
      local ok, txErr = pcall(function()
        local state = catalog:withWriteAccessDo("Mimic apply " .. tostring(item.predictionId), function()
          outcome = applyOne(catalog, photo, item, createSnapshot, readBack)
        end, { timeout = 15 })
        if state ~= "executed" then
          outcome = { photoId = item.photoId, predictionId = item.predictionId, status = "failed", error = { code = "catalog_write_denied", message = "withWriteAccessDo returned " .. tostring(state) } }
        end
      end)
      if not ok then
        outcome = { photoId = item.photoId, predictionId = item.predictionId, status = "failed", error = { code = "catalog_write_error", message = tostring(txErr) } }
      end
      results[#results + 1] = outcome
    end
  end
  Logger.info("apply batch finished", { items = #items, results = #results, canceled = canceled })
  return { ok = true, result = { items = Json.array(results), canceled = canceled } }
end

return Develop
