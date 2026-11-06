// question: waittillmatch({#name:"x"}, e) ignores a notify of e whose payload field differs and wakes on the matching one
// fact: waittillmatch({#name:"x"}, e) skips a notify whose payload field differs and wakes, inside the notify, on the matching one
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T0 sent y
// expect: f1 T1 matched x
// expect: f1 T0 sent x
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e", {#name:"y"});
	iprintln("sent y");
	level notify(#"e", {#name:"x"});
	iprintln("sent x");
}

function waiter()
{
	s = level waittillmatch({#name:"x"}, #"e");
	iprintln("matched " + s.name);
}
