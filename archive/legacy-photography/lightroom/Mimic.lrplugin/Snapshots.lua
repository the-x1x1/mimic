-- Develop snapshot helpers (recovery path, spec §2.4).

local Snapshots = {}

function Snapshots.create(catalog, photo, name)
  if type(photo.createDevelopSnapshot) ~= "function" then
    return false, { code = "unsupported", message = "createDevelopSnapshot is not available in this Lightroom" }
  end
  local outcome, err
  local ok, txErr = pcall(function()
    local state = catalog:withWriteAccessDo("Mimic snapshot", function()
      local okS, e = pcall(function() photo:createDevelopSnapshot(name, true) end)
      outcome = okS
      err = e
    end, { timeout = 10 })
    if state ~= "executed" then
      outcome = false
      err = "withWriteAccessDo returned " .. tostring(state)
    end
  end)
  if not ok then return false, { code = "catalog_write_error", message = tostring(txErr) } end
  if not outcome then return false, { code = "snapshot_failed", message = tostring(err) } end
  return true
end

return Snapshots
