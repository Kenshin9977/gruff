// question: does a childthread stop when the thread that started it ends?
// unsupported: T7 has no childthread: the official linker reads `childthread` as an uninitialised local variable and refuses the script
// expect compile error: Uninitialized local variable 'childthread'
#namespace zz_test;

function main()
{
	thread parent();
}

function parent()
{
	logprint("parent start");
	childthread child();
	wait 0.1;
	logprint("parent end");
}

function child()
{
	for (i = 0; i < 10; i++)
	{
		logprint("child tick " + i);
		wait 0.05;
	}
}
