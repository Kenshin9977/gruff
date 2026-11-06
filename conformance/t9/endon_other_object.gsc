// question: an endon on a struct is not fired by the same event on level (and the reverse)
// fact: an endon is bound to its object
// vs t8: same
// expect: f0 T1 waiting
// expect: f1 T0 level sent
// expect: f2 T0 struct sent
#namespace zz_test;

function main()
{
	s = spawnstruct();
	level.s = s;
	thread waiter(s);
	wait 0.05;
	level notify(#"stop");
	iprintln("level sent");
	wait 0.05;
	s notify(#"stop");
	iprintln("struct sent");
	wait 0.05;
	level notify(#"go");
}

function waiter(s)
{
	s endon(#"stop");
	iprintln("waiting");
	level waittill(#"go");
	iprintln("woke (only if alive)");
}
