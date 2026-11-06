/@
"Name: tally( <count>, [step] )"
"Summary: adds things up"
@/
function private tally( count, step = 1, &out, ... )
{
	total = 0;
	for ( i = 0; i < count; i += step )
	{
		total += i;
	}
	out = total;
	return total + vararg.size;
}

function autoexec boot()
{
	level.booted = true;
}

function codecall on_engine_event( a, b )
{
}
