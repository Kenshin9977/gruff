// question: x = thread f(): accepted, and what does x hold when f returns before or after its first wait?
// fact: x = thread f() compiles (acts); x is f's return value when f ends without waiting, undefined when f waits first
// vs t7: same
// expect: f0 T0 x defined=0
// expect: f0 T0 y=6
#namespace zz_test;

function five()
{
	wait 0.05;
	return 5;
}

function six()
{
	return 6;
}

function main()
{
	x = thread five();
	iprintln("x defined=" + isdefined(x));
	y = thread six();
	iprintln("y=" + y);
}
