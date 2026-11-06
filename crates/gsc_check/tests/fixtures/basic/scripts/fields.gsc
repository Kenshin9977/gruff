#namespace fields_demo;

function f()
{
	self.counter = 0;
	if ( self.helath < 50 )        // a typo of the engine's `health`
	{
		self.countre++;              // a write: never reported
	}
	if ( isdefined( self.optional_flag ) && self.optional_flag )
	{
		x = self.optional_flag;
	}
	y = self.coutner;              // a typo of `counter`, set above
	z = self.offset_y + self.offset_x;
	self.offset_x = 1;
	w = self.unrelated_name_nobody_sets;
}
