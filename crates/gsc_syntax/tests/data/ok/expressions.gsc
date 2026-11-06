function exprs( a, b )
{
	x = a + b * 2 - ( a - b ) / 3 % 4;
	y = a < b && b <= a || !( a >= b ) && a === b || a !== b;
	z = a ? b : a ? 1 : 2;
	bits = a & b | a ^ ~b << 2 >> 1;
	v = ( 1, 2.5, -3 );
	arr = [];
	arr[ arr.size ] = #"hashed";
	label = &"DEMO_LABEL";
	anim_ref = %demo_anim_idle;
	tree = #animtree;
	f = &helper;
	g = &demo_ns::helper;
	self thread helper( 1 );
	self demo_ns::helper();
	thread helper();
	level.ents[ 0 ] notify( "go", a );
	self waittill( "done", result );
	[[ f ]]( a );
	self [[ level.callbacks[ "spawn" ] ]]();
	thread [ [ g ] ]();
	result = self getsomething().field;
	obj = new demo_class();
	[[ obj ]]->run( 1, 2 );
	count = isdefined( a ) + istrue_like( b );
	a++;
	--b;
	a += -b;
}
