// File header comment.
/* Block header,
   two lines. */

function f(a, // first parameter
	b)
{ // after the brace
	x = 1;		// aligned trailing comment
	y = 22;		// aligned trailing comment

	// own-line comment before a statement


	// after two blank lines
	foo(a, /* inline */ b);
	bar(
		// own-line comment inside the arguments
		a,
		b // trailing the last argument
	);
	if (x) // trailing the condition
	{
		z = 3;
	}
	// before else
	else
	{
		z = 4;
	}
	/# println("dev"); /* inside dev */ #/
	/#
		// comment inside a dev block
		iprintln(x);
	#/
	w = a /* before the operator */ + b;
	// last comment in the block
}
// between functions

/* block between functions */
function g()
{
	// only a comment
}
// trailing comment of the file
