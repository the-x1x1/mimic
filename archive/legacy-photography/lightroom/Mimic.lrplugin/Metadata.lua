-- Formatted metadata helper (display values) — used by get_photo_metadata.

local Metadata = {}

local FORMATTED = { "fileName", "folderName", "cameraMake", "cameraModel", "lens", "isoSpeedRating", "aperture", "shutterSpeed", "focalLength", "dateTimeOriginal", "dimensions", "croppedDimensions", "title", "caption", "keywordTags", "label", "rating" }

function Metadata.formatted(photo)
  local out = {}
  for _, key in ipairs(FORMATTED) do
    local ok, v = pcall(function() return photo:getFormattedMetadata(key) end)
    if ok and v ~= nil and v ~= "" then out[key] = v end
  end
  return out
end

return Metadata
