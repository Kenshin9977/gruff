// question: the return value of a function called normally, and what a threaded call leaves to its caller
// fact: a direct call returns its value; a threaded call as a statement drops it
// vs t8: same
// expect: f0 T0 direct=5
// expect: f0 T0 threaded call done
#namespace zz_test;

function main()
{
	x = five();
	iprintln("direct=" + x);
	thread five();
	iprintln("threaded call done");
}

function five()
{
	return 5;
}
