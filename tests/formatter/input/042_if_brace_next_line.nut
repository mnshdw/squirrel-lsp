function onDamageReceived(_attacker, _damageHitpoints, _damageArmor) {
	if (
		_damageHitpoints >= getContainer().getActor().getHitpoints()
			|| !m.WasHitBySkill
			|| m.IsPerformingTeleportation)
		{
			return;
		}

		m.DummyVariable = getContainer().getActor().m.IsAttackable;
		m.IsPerformingTeleportation = true;
		getContainer().getActor().m.IsAttackable = false;
		::Time.scheduleEvent(::TimeUnit.Virtual, 30, teleport.bindenv(this), getContainer().getActor());
	}
