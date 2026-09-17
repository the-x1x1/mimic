--[[
  Command handlers. Keys match mimic-core::bridge::CommandType exactly.
  Each handler receives (catalog, payload, bridge) and returns either a plain
  result table or { ok = false, error = {...} }.
]]

local Json = require "Json"
local Catalog = require "Catalog"
local Develop = require "Develop"
local Snapshots = require "Snapshots"
local Metadata = require "Metadata"
local Capabilities = require "Capabilities"

local Commands = {}

function Commands.ping(_catalog, payload)
  return { pong = true, echo = payload.echo, at = os.time() }
end

function Commands.get_catalog_info(catalog)
  return Catalog.info(catalog)
end

function Commands.get_capabilities(catalog)
  return Capabilities.probe(catalog)
end

function Commands.get_selected_photos(catalog, payload)
  local photos, total = Catalog.photosForScope(catalog, payload.scope or "selection", payload.maxPhotos)
  local out = Json.array()
  for _, p in ipairs(photos) do out[#out + 1] = Catalog.photoMetadata(p) end
  return { scope = payload.scope or "selection", photos = out, total = total, truncated = total > #photos }
end

function Commands.get_photo_metadata(catalog, payload)
  local items = Json.array()
  for _, id in ipairs(payload.photoIds or {}) do
    local photo = Catalog.findPhotoById(catalog, id)
    if photo then
      local m = Catalog.photoMetadata(photo)
      m.formatted = Metadata.formatted(photo)
      items[#items + 1] = m
    else
      items[#items + 1] = { photoId = id, error = { code = "photo_not_found", message = "no photo " .. tostring(id) } }
    end
  end
  return { items = items }
end

local function settingsFor(catalog, ids)
  local items = Json.array()
  for _, id in ipairs(ids or {}) do
    local photo = Catalog.findPhotoById(catalog, id)
    if not photo then
      items[#items + 1] = { photoId = id, error = { code = "photo_not_found", message = "no photo " .. tostring(id) } }
    else
      local settings, err = Develop.getSettings(photo)
      if settings then
        items[#items + 1] = { photoId = id, path = photo:getRawMetadata("path"), settings = settings }
      else
        items[#items + 1] = { photoId = id, error = { code = "get_develop_settings_failed", message = tostring(err) } }
      end
    end
  end
  return { items = items }
end

function Commands.get_develop_settings(catalog, payload)
  return settingsFor(catalog, payload.photoIds)
end

function Commands.read_back_develop_settings(catalog, payload)
  return settingsFor(catalog, payload.photoIds)
end

function Commands.collect_correction_state(catalog, payload)
  -- Same read as read_back; the desktop diffs against what it applied.
  local out = settingsFor(catalog, payload.photoIds)
  out.collectedAt = os.date("!%Y-%m-%dT%H:%M:%SZ")
  return out
end

function Commands.create_before_snapshot(catalog, payload)
  local items = Json.array()
  for _, item in ipairs(payload.items or {}) do
    local photo = Catalog.findPhotoById(catalog, item.photoId)
    if not photo then
      items[#items + 1] = { photoId = item.photoId, status = "failed", error = { code = "photo_not_found", message = "no photo " .. tostring(item.photoId) } }
    else
      local ok, err = Snapshots.create(catalog, photo, item.snapshotName or ("Mimic Before — " .. os.date("!%Y-%m-%dT%H:%M:%SZ")))
      if ok then
        items[#items + 1] = { photoId = item.photoId, status = "created", snapshotName = item.snapshotName }
      else
        items[#items + 1] = { photoId = item.photoId, status = "failed", error = err }
      end
    end
  end
  return { items = items }
end

function Commands.apply_settings_as_plugin_preset(catalog, payload, bridge)
  return Develop.applyBatch(catalog, payload, bridge, function(id) return Catalog.findPhotoById(catalog, id) end)
end

return Commands
