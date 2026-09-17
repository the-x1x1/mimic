--[[
  Minimal, dependency-free JSON encoder/decoder for the Lightroom Lua runtime
  (Lua 5.1 semantics; no goto, no bit ops). Handles objects, arrays, strings
  (with \uXXXX and surrogate pairs), numbers, booleans and null.

  Arrays vs objects: a Lua table is encoded as an array when it has only
  consecutive integer keys starting at 1 (or is marked with Json.array()).
  Json.null is a sentinel that encodes as `null`.
]]

local Json = {}

Json.null = setmetatable({}, { __tostring = function() return "null" end })

local ARRAY_MT = { __jsontype = "array" }
local OBJECT_MT = { __jsontype = "object" }

function Json.array(t) return setmetatable(t or {}, ARRAY_MT) end
function Json.object(t) return setmetatable(t or {}, OBJECT_MT) end

local escapes = { ['"'] = '\\"', ['\\'] = '\\\\', ['\b'] = '\\b', ['\f'] = '\\f', ['\n'] = '\\n', ['\r'] = '\\r', ['\t'] = '\\t' }

local function escapeString(s)
  return s:gsub('[%c"\\]', function(c)
    return escapes[c] or string.format('\\u%04x', c:byte())
  end)
end

local function isArray(t)
  local mt = getmetatable(t)
  if mt == ARRAY_MT then return true end
  if mt == OBJECT_MT then return false end
  local n = 0
  for k in pairs(t) do
    if type(k) ~= "number" or k < 1 or k % 1 ~= 0 then return false end
    n = n + 1
  end
  if n == 0 then return false end
  for i = 1, n do
    if t[i] == nil then return false end
  end
  return true
end

local encodeValue

local function encodeTable(t, depth)
  if depth > 64 then error("Json.encode: nesting too deep") end
  local parts = {}
  if isArray(t) then
    for i = 1, #t do parts[#parts + 1] = encodeValue(t[i], depth + 1) end
    return "[" .. table.concat(parts, ",") .. "]"
  end
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = tostring(k) end
  table.sort(keys)
  for _, k in ipairs(keys) do
    local v = t[k]
    if v == nil then v = t[tonumber(k)] end
    parts[#parts + 1] = '"' .. escapeString(k) .. '":' .. encodeValue(v, depth + 1)
  end
  return "{" .. table.concat(parts, ",") .. "}"
end

encodeValue = function(v, depth)
  local tv = type(v)
  if v == Json.null or v == nil then return "null" end
  if tv == "boolean" then return v and "true" or "false" end
  if tv == "number" then
    if v ~= v or v == math.huge or v == -math.huge then return "null" end
    if v % 1 == 0 and math.abs(v) < 1e15 then return string.format("%d", v) end
    return string.format("%.14g", v)
  end
  if tv == "string" then return '"' .. escapeString(v) .. '"' end
  if tv == "table" then return encodeTable(v, depth or 0) end
  return '"' .. escapeString(tostring(v)) .. '"'
end

function Json.encode(v) return encodeValue(v, 0) end

-- Decoder -------------------------------------------------------------------

local function decodeError(str, pos, msg)
  error(string.format("Json.decode: %s at position %d (near %q)", msg, pos, str:sub(pos, pos + 15)), 0)
end

local function skipWs(str, pos)
  local _, e = str:find("^[ \n\r\t]*", pos)
  return e + 1
end

local function utf8Char(cp)
  if cp < 0x80 then return string.char(cp) end
  if cp < 0x800 then return string.char(0xC0 + math.floor(cp / 0x40), 0x80 + cp % 0x40) end
  if cp < 0x10000 then
    return string.char(0xE0 + math.floor(cp / 0x1000), 0x80 + math.floor(cp / 0x40) % 0x40, 0x80 + cp % 0x40)
  end
  return string.char(0xF0 + math.floor(cp / 0x40000), 0x80 + math.floor(cp / 0x1000) % 0x40, 0x80 + math.floor(cp / 0x40) % 0x40, 0x80 + cp % 0x40)
end

local unescapes = { b = "\b", f = "\f", n = "\n", r = "\r", t = "\t", ['"'] = '"', ["\\"] = "\\", ["/"] = "/" }

local decodeValue

local function decodeString(str, pos)
  local out = {}
  local i = pos + 1
  while true do
    local c = str:sub(i, i)
    if c == "" then decodeError(str, pos, "unterminated string") end
    if c == '"' then return table.concat(out), i + 1 end
    if c == "\\" then
      local n = str:sub(i + 1, i + 1)
      if n == "u" then
        local hex = str:sub(i + 2, i + 5)
        if not hex:match("^%x%x%x%x$") then decodeError(str, i, "bad unicode escape") end
        local cp = tonumber(hex, 16)
        i = i + 6
        if cp >= 0xD800 and cp <= 0xDBFF and str:sub(i, i + 1) == "\\u" then
          local lo = tonumber(str:sub(i + 2, i + 5), 16)
          if lo and lo >= 0xDC00 and lo <= 0xDFFF then
            cp = 0x10000 + (cp - 0xD800) * 0x400 + (lo - 0xDC00)
            i = i + 6
          end
        end
        out[#out + 1] = utf8Char(cp)
      else
        local u = unescapes[n]
        if not u then decodeError(str, i, "bad escape") end
        out[#out + 1] = u
        i = i + 2
      end
    else
      out[#out + 1] = c
      i = i + 1
    end
  end
end

local function decodeNumber(str, pos)
  local s, e = str:find("^-?%d+%.?%d*[eE]?[-+]?%d*", pos)
  if not s then decodeError(str, pos, "bad number") end
  local n = tonumber(str:sub(s, e))
  if n == nil then decodeError(str, pos, "bad number") end
  return n, e + 1
end

local function decodeArray(str, pos)
  local out = Json.array()
  pos = skipWs(str, pos + 1)
  if str:sub(pos, pos) == "]" then return out, pos + 1 end
  while true do
    local v
    v, pos = decodeValue(str, pos)
    out[#out + 1] = v
    pos = skipWs(str, pos)
    local c = str:sub(pos, pos)
    if c == "]" then return out, pos + 1 end
    if c ~= "," then decodeError(str, pos, "expected , or ]") end
    pos = skipWs(str, pos + 1)
  end
end

local function decodeObject(str, pos)
  local out = Json.object()
  pos = skipWs(str, pos + 1)
  if str:sub(pos, pos) == "}" then return out, pos + 1 end
  while true do
    if str:sub(pos, pos) ~= '"' then decodeError(str, pos, "expected string key") end
    local k
    k, pos = decodeString(str, pos)
    pos = skipWs(str, pos)
    if str:sub(pos, pos) ~= ":" then decodeError(str, pos, "expected :") end
    pos = skipWs(str, pos + 1)
    local v
    v, pos = decodeValue(str, pos)
    out[k] = v
    pos = skipWs(str, pos)
    local c = str:sub(pos, pos)
    if c == "}" then return out, pos + 1 end
    if c ~= "," then decodeError(str, pos, "expected , or }") end
    pos = skipWs(str, pos + 1)
  end
end

decodeValue = function(str, pos)
  pos = skipWs(str, pos)
  local c = str:sub(pos, pos)
  if c == "{" then return decodeObject(str, pos) end
  if c == "[" then return decodeArray(str, pos) end
  if c == '"' then return decodeString(str, pos) end
  if c == "-" or c:match("%d") then return decodeNumber(str, pos) end
  if str:sub(pos, pos + 3) == "true" then return true, pos + 4 end
  if str:sub(pos, pos + 4) == "false" then return false, pos + 5 end
  if str:sub(pos, pos + 3) == "null" then return Json.null, pos + 4 end
  decodeError(str, pos, "unexpected character")
end

function Json.decode(str)
  if type(str) ~= "string" then error("Json.decode: expected string", 0) end
  local v, pos = decodeValue(str, 1)
  pos = skipWs(str, pos)
  if pos <= #str then decodeError(str, pos, "trailing garbage") end
  return v
end

return Json
