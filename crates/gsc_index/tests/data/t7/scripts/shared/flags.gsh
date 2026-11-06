// Fixture written for gsc_index tests.
#define IS_ON(__f) ( isdefined( __f ) && __f )
#define TICK 0.05
#define BURN_OUT(__id) level notify( "beacon_out", __id ); level.beacon_count--; relay::send( "out_" + __id )
