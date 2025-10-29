// EXPECT: no errors
::Mod_ROTU.HookHelper <- {
    modifyDefenderResources = function( _inputResources, _mult = 1.0, _includeBoost = true, _scale = true, _includeDefaultMult = true ) {
        return this.modifyResources(_inputResources, _mult, _includeBoost, _scale) * (_includeDefaultMult ? ::Mod_ROTU.Const.DefenderResoursesMult : 1.0);
    }

    modifyResources = function( _inputResources, _mult = 1.0, _includeBoost = true, _scale = true ) {
        return _inputResources * _mult;
    }
}

local obj = {
    value = 5
    increment = function() {
        this.value = this.value + 1;
        return this.value;
    }
}
