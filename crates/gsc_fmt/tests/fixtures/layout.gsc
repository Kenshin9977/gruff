#using scripts\shared\util;

function autoexec init(a,b=5,&c)
{
x=a+b ;
if(isdefined(self.target_entity)&&self.target_entity.health>0&&isdefined(level.boss_fight_phase)&&!level.paused) self thread attack();
else if(x) return;
else {
y = 2;
}
self thread spawn_wave_of_enemies(level.spawn_points[0],"zombie_wave_group_alpha",1.5,(0,0,1),undefined,self.origin,5000);
message = "The quick brown fox " + "jumps over the lazy dog " + "and keeps running " + "until the end of the line" + x;
value = (isdefined(self.override_value_for_testing) ? self.override_value_for_testing : level.default_value_for_testing);
short = a ? b : c;
list = [ "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda" ];
point = {#x:1, #y:2, #z:3};
grid[i][j] = level.cells[ i * 8 + j ];
foreach(p in level.players) p notify("tick");
for(i=0;i<10;i++){ wait 0.05; }
for(;;) break;
while(!done) done = check();
do { n++; } while(n<5);
switch(mode){
case "a":
case "b": run(); break;
default:
{
idle();
}
}
level.callbacks[ "on_player_spawned_for_the_first_time" ] = &on_player_spawned_for_the_first_time_handler;
[[ level.func ]](a, b);
self [[ level.method ]]();
if(!(isdefined(level.a) && level.a)) { return; }
x = -y + ~z - (-w);
i++; --j;
}



function empty() {}
