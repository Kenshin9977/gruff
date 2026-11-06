// Self-written fixture for the event rules (T001-T004).
#namespace round;

function start()
{
    level notify( "round_started" );
    level notify( "wave_" + level.wave );
    self notify( "picked_up", self.item );
    level thread delayed( "late_event" );
}

function delayed( msg )
{
    wait 1;
    level notify( msg );
}

function waits()
{
    level waittill( "round_startd" );
    level waittill( "round_started" );
    level waittill( "wave_3" );
    level waittill( "late_event" );
    self waittill( "picked_up", item, player );
    self waittill( "death", attacker );
    self endon( "dissconnect" );
    self endon( "disconnect" );
    self waittill( "never_sent_anywhere" );
}

function spawned( player )
{
    player thread hud_loop();
    player thread hud_loop_ok();
    level thread level_loop();
}

function hud_loop()
{
    while ( 1 )
    {
        self.hud_value = self.score;
        wait 0.5;
    }
}

function hud_loop_ok()
{
    self endon( "disconnect" );
    while ( 1 )
    {
        self.hud_value = self.score;
        wait 0.5;
    }
}

function level_loop()
{
    while ( 1 )
    {
        wait 1;
    }
}
