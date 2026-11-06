// question: a thread that both endon()s and waittill()s the same event: woken or killed?
// fact: endon and waittill on the same event: killed, not woken
// vs t8: same
// expect: f0 T1 waiting
// expect: f1 T0 sent
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e");
	iprintln("sent");
}

function waiter()
{
	level endon(#"e");
	iprintln("waiting");
	level waittill(#"e");
	iprintln("woke");
}
