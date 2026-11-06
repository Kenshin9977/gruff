#namespace client;

function main( localclientnum )
{
	// The server form of getent: the client one takes the client number first.
	ent = getent( "x", "targetname" );
	ent setmodel( "y" );
	o = spawn( localclientnum, ( 0, 0, 0 ), "script_model" );
	playfx( localclientnum, "fx", ( 0, 0, 0 ) );
}
