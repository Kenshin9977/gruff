// question: a waittill on a struct is not woken by the same event on level, and the reverse
// fact: a waittill is not woken by the same event on another object
// vs t7: same
// expect: f0 T1 struct waiter
// expect: f0 T2 level waiter
// expect: f1 T0 level sent
// expect: f1 T2 level woke
// expect: f2 T0 struct sent
// expect: f2 T1 struct woke
#namespace zz_test;

function main()
{
	s = spawnstruct();
	thread on_struct(s);
	thread on_level();
	wait 0.05;
	level notify(#"e");
	iprintln("level sent");
	wait 0.05;
	s notify(#"e");
	iprintln("struct sent");
}

function on_struct(s)
{
	iprintln("struct waiter");
	s waittill(#"e");
	iprintln("struct woke");
}

function on_level()
{
	iprintln("level waiter");
	level waittill(#"e");
	iprintln("level woke");
}
