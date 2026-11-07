// question: the return value of a function called normally, and what a threaded call leaves to its caller
// expect: f0 T0 direct=5
// expect: f0 T0 threaded call done
// fact: a direct call returns its value; a threaded call used as a statement drops whatever it leaves
// vs iw3: same

main()
{
	x = five();
	logprint("direct=" + x);
	thread five();
	logprint("threaded call done");
}

five()
{
	return 5;
}
