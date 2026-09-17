-- Run with: lua5.1 tests/json_test.lua (from lightroom/). CI runs this.
package.path = "./Mimic.lrplugin/?.lua;" .. package.path
local Json = require("Json")

local function eq(a, b, msg) if a ~= b then error((msg or "mismatch") .. ": " .. tostring(a) .. " ~= " .. tostring(b), 2) end end

-- roundtrip of the checked-in bridge fixtures
local fixtures = { "handshake.request.json", "handshake.response.json", "get_develop_settings.result.json", "apply_settings_as_plugin_preset.command.json", "apply_settings_as_plugin_preset.result.partial_failure.json", "command_error.result.json", "event.selection_changed.json" }
for _, name in ipairs(fixtures) do
  local f = assert(io.open("../fixtures/bridge/" .. name, "rb"))
  local text = f:read("*a"); f:close()
  local v = Json.decode(text)
  local again = Json.decode(Json.encode(v))
  eq(Json.encode(again), Json.encode(v), name .. " not stable")
end

local d = Json.decode(io.open("../fixtures/bridge/get_develop_settings.result.json", "rb"):read("*a"))
eq(d.result.settings.Exposure2012, 0.35, "float")
eq(d.result.settings.Temperature, 5350, "int")
eq(d.result.settings.WhiteBalance, "Custom", "string")
eq(d.result.settings.HasCrop, false, "bool")
eq(#d.result.settings.ToneCurvePV2012, 4, "curve length")
eq(d.result.settings.ToneCurvePV2012[2][2], 58, "curve point")
eq(Json.encode(d.result.settings.MaskGroupBasedCorrections), "[]", "empty array preserved")

-- edge cases
eq(Json.encode({}), "{}", "empty table is object")
eq(Json.encode(Json.array()), "[]", "explicit array")
eq(Json.encode({ [1] = "a", [2] = "b" }), '["a","b"]', "sequence")
eq(Json.encode({ x = Json.null }), '{"x":null}', "null")
eq(Json.encode("tab\tnl\n\"q\""), '"tab\\tnl\\n\\"q\\""', "escapes")
eq(Json.decode('"\\u00e9"'), "é", "unicode")
eq(Json.decode("  [1, 2.5, -3e2, true, null] ")[3], -300, "numbers")
eq(Json.decode("1e3"), 1000, "exp")
local ok = pcall(Json.decode, "{bad json}")
eq(ok, false, "malformed must error")
local ok2 = pcall(Json.decode, "[1,2] trailing")
eq(ok2, false, "trailing garbage must error")
-- nested structure with mixed keys
local nested = Json.decode('{"a":{"b":[{"c":1},{"c":2}]},"n":-0.5}')
eq(nested.a.b[2].c, 2, "nested")
eq(nested.n, -0.5, "negative float")
print("json_test: ok")
