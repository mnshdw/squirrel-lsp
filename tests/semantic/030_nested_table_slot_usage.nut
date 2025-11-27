// EXPECT: no errors
// Test that nested table slots accessed via this.m.x are not flagged as unused

this.skill <- {
    m = {
        offHandSkill = null,
        HandToHand = null
    },

    function setOffhandSkill(_a) {
        this.m.offHandSkill = _a;
    },

    function getHandToHand() {
        return this.m.HandToHand;
    }
}
