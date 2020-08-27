<?php
$CONFIG = array (
  'htaccess.RewriteBase' => '/nextcloud',
  'memcache.local' => '\\OC\\Memcache\\APCu',
  'memcache.distributed' => '\\OC\\Memcache\\Redis',
  'memcache.locking' => '\\OC\\Memcache\\Redis',
  'redis' => [
      'host' => 'redis',
      'port' => 6379,
  ],
  "appstoreenabled" => false,
  'apps_paths' =>
  array (
    0 =>
    array (
      'path' => '/var/www/nextcloud/apps',
      'url' => '/apps',
      'writable' => false,
    ),
    1 =>
    array (
      'path' => '/var/www/nextcloud/custom_apps',
      'url' => '/custom_apps',
      'writable' => false,
    ),
  ),
  'passwordsalt' => 'hKTTwGw3cdFxaLOJ05C139QOvoqcOC',
  'secret' => 'cIEb3oY907NWS/YEfdSQn/gGw8iJyluul6kdvkOPbQg1vziv',
  'trusted_domains' =>
  array (
    0 => 'localhost',
    1 => 'localhost',
    2 => 'moellenstedt.iff.kfa-juelich.de',
    3 => '134.94.162.235',
  ),
  'datadirectory' => '/var/local/nextclouddata',
  'dbtype' => 'pgsql',
  'version' => '19.0.1.1',
  'overwrite.cli.url' => 'http://moellenstedt.iff.kfa-juelich.de/nextcloud',
  'dbname' => 'nextcloud',
  'dbhost' => 'db',
  'dbport' => '',
  'dbtableprefix' => 'oc_',
  'dbuser' => 'oc_root',
  'dbpassword' => 'iw87kzj8spseqfhjhcy3weaakefr5w',
  'installed' => true,
  'instanceid' => 'ociwsxys2w4h',
  'ldapIgnoreNamingRules' => false,
  'ldapProviderFactory' => 'OCA\\User_LDAP\\LDAPProviderFactory',
);