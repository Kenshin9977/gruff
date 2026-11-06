// question: waittill binds two names, the notify sends one value: the second stays undefined?
// fact: waittill names with no value sent stay undefined
// expect: f1 T0 sent
// expect: f1 T1 a=1 b defined=0
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify("e", 1);
	logprint("sent");
}

function waiter()
{
	level waittill("e", a, b);
	logprint("a=" + a + " b defined=" + isdefined(b));
}
