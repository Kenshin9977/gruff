// question: waittilltimeout(0.1, e) with no notify: when does it return and what is _notify?
// fact: waittilltimeout(0.1, e) with no notify returns after 3 frames (150 ms, not 100), with _notify "timeout"
// vs t7: T7 has no waittilltimeout
// evidence: static: VM_SetTime (0x2773570) fires a timeout only when the VM time is past its deadline (jbe skips equal, 0x2773760), so a 100 ms timeout fires at the 150 ms frame
// expect: f0 T0 start
// expect: f3 T0 returned, timeout=1
#namespace zz_test;

function main()
{
	iprintln("start");
	s = level waittilltimeout(0.1, #"never");
	iprintln("returned, timeout=" + (s._notify == "timeout"));
}
