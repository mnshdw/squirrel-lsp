class Config {

	function save() {
		local settings = {
			fullscreen = FULL_SCREEN
			bgm_volume = BGM_VOLUME
			se_volume = SE_VOLUME
		}
		local f = file("config.nut", "w")
		foreach (key, value in settings) {
			if (typeof value == "string") {
				value = "\"" + value + "\""
			}
			local line = key + " <- " + value + "\n"
			foreach (char in line) {
				f.writen(char, 'c')
			}
		}
		f.close()
	}

	function load() {
		local configScript = loadfile("config.nut")
		configScript.bindenv(settings)()

		FULL_SCREEN = settings.fullscreen
		BGM_VOLUME = settings.bgm_volume.tofloat()
	}
}
