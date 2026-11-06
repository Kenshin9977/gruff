// question: waittilltimeout(0.1, e) with no notify: when does it return and what is _notify?
// fact: waittilltimeout(0.1, e) with no notify returns after 2 frames (100 ms), with _notify "timeout"
// vs t8: T8 returns after 3 frames (its timeouts fire only past the deadline)
// evidence: static: VM_SetTime (0x1bb4c90) fires a timeout entry when the VM time is >= its deadline (comisd + jb at 0x1bb4dff); T8's 0x2773760 uses jbe (strictly after)
// expect: f0 T0 start
// expect: f2 T0 returned, timeout=1
#namespace zz_test;

function main()
{
	iprintln("start");
	s = level waittilltimeout(0.1, #"never");
	iprintln("returned, timeout=" + (s._notify == "timeout"));
}
