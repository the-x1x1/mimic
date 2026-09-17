-- Read-only catalog helpers: photo listing and metadata.

local LrApplication = import "LrApplication"

local Catalog = {}

local RAW_META = { "path", "uuid", "fileSize", "dateTimeOriginal", "cameraMake", "cameraModel", "lens", "isoSpeedRating", "aperture", "shutterSpeed", "focalLength", "width", "height", "orientation", "isVirtualCopy", "copyName", "rating", "pickStatus" }

function Catalog.photoMetadata(photo)
  local m = {}
  for _, key in ipairs(RAW_META) do
    local ok, v = pcall(function() return photo:getRawMetadata(key) end)
    if ok and v ~= nil then m[key] = v end
  end
  local out = {
    photoId = photo.localIdentifier,
    path = m.path,
    uuid = m.uuid,
    isVirtualCopy = m.isVirtualCopy == true,
    copyName = m.copyName,
    metadata = {
      cameraMake = m.cameraMake,
      cameraModel = m.cameraModel,
      lens = m.lens,
      iso = m.isoSpeedRating,
      aperture = m.aperture,
      shutterSpeed = m.shutterSpeed,
      focalLength = m.focalLength,
      width = m.width,
      height = m.height,
      orientation = m.orientation,
      rating = m.rating,
      pickStatus = m.pickStatus,
      sizeBytes = m.fileSize,
    },
  }
  if m.dateTimeOriginal then
    -- Lightroom returns seconds since 2001-01-01 (Cocoa epoch).
    local unix = m.dateTimeOriginal + 978307200
    out.metadata.capturedAt = os.date("!%Y-%m-%dT%H:%M:%S", unix)
  end
  return out
end

-- scope: selection | collection | folder | catalog
function Catalog.photosForScope(catalog, scope, maxPhotos)
  local photos
  if scope == "catalog" then
    photos = catalog:getAllPhotos()
  elseif scope == "collection" or scope == "folder" then
    local sources = catalog:getActiveSources()
    photos = {}
    for _, src in ipairs(sources or {}) do
      local ok, list = pcall(function() return src:getPhotos(scope == "folder") end)
      if ok and list then
        for _, p in ipairs(list) do photos[#photos + 1] = p end
      end
    end
    if #photos == 0 then photos = catalog:getTargetPhotos() end
  else
    photos = catalog:getTargetPhotos()
  end
  local limit = tonumber(maxPhotos) or 50000
  local out = {}
  for i, p in ipairs(photos) do
    if i > limit then break end
    out[#out + 1] = p
  end
  return out, #photos
end

function Catalog.findPhotoById(catalog, id)
  local ok, photo = pcall(function() return catalog:getPhotoByLocalId(id) end)
  if ok and photo then return photo end
  return nil
end

function Catalog.info(catalog)
  local v = LrApplication.versionTable()
  return {
    path = catalog:getPath(),
    lightroomVersion = v and string.format("%d.%d.%d", v.major, v.minor, v.revision) or "unknown",
    hasWriteAccess = catalog.hasWriteAccess and catalog:hasWriteAccess() or nil,
  }
end

return Catalog
