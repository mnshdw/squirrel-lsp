q.onUnequip = @(__original)function() {
	__original();
	this.m.IsHidden = false;
};

q.onEquip = @( __original ) function( ) {
__original( );
this.m.IsHidden=true;
};