// question: waittillmatch(e, "x") ignores a notify of e with another first argument and wakes on "x"
// expect: f1 T0 sent y
// expect: f1 T0 sent x
// expect: f1 T1 matched
// fact: waittillmatch skips notifies whose first argument differs and wakes on the matching one
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify("e", "y");
	logprint("sent y");
	level notify("e", "x");
	logprint("sent x");
}

function waiter()
{
	level waittillmatch("e", "x");
	logprint("matched");
}
