-- Plugin entry: start the bridge loop once Lightroom has loaded the plugin.
local Bridge = require "Bridge"
local Logger = require "Logger"
local Version = require "Version"

Logger.info("Mimic plugin loaded", { version = Version.version })
Bridge.start()
