// EXPECT: no errors
local statusDamageMultiplier = 1;
local damageMult = 1.0;
local hitInfo = clone this.Const.Tactical.HitInfo;
local mainhand = this.getContainer().getActor().getItems().getItemAtSlot(this.Const.ItemSlot.Mainhand);
if (mainhand != null) {
    local isWeakened = this.isWeakenedBlessing();
    local missDamageScale = (isWeakened ? this.m.MissDamageWeakened : this.m.MissDamageFull) / 100;
    local damage = this.Math.rand(Math.round(mainhand.getDamageMin()) * missDamageScale, Math.round(mainhand.getDamageMax() * missDamageScale));
    hitInfo.DamageRegular = Math.minf(damage * damageMult, 60 * statusDamageMultiplier);
    hitInfo.DamageArmor = Math.minf(damage * damageMult, 60 * statusDamageMultiplier);
}
