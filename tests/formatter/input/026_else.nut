if (days < 50)
	_properties.DamageTotalMult *= 0.9;
else if (days <= 100)
	_properties.DamageTotalMult *= 0.85;
else
	_properties.DamageTotalMult *= 0.8;