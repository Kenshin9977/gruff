// question: waittillmatch({#name:"x"}, e) ignores a notify of e whose payload field differs and wakes on the matching one
// fact: waittillmatch({#name:"x"}, e) skips a notify whose payload field differs and wakes on the matching one, with that payload
// vs t7: T7 waittillmatch compares the first notify value with a string
// evidence: static: VM_Notify (0x2774090) compares a waittillmatch entry's fields with the payload (0x275e430) before waking it
// expect: f1 T0 sent y
// expect: f1 T0 sent x
// expect: f1 T1 matched x
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
