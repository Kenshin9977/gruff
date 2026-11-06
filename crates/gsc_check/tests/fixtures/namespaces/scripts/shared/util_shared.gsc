#namespace util;

function wait_network_frame()
{
	wait 0.05;
}

// Shares a builtin's name: a bare call elsewhere still reaches the builtin.
function spawn( classname )
{
	return undefined;
}

function in_two_namespaces()
{
}
