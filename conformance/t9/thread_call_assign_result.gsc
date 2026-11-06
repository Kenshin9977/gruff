// question: x = thread f(): accepted, and what does x hold when f returns before or after its first wait?
// fact: x = thread f() is not f's return value but a value of the VM type the game names 'finalization' (type 7): defined even when f waits, and concatenating it with a string is a script error (code #12e690eb) that yields undefined
// vs t8: T8 and T7: x is f's return value when f ends without waiting, undefined when f waits first
// expect: f0 T0 x defined=1
// expect: f0 T0 script error: #12e690eb
// expect: f0 T0 undefined
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
