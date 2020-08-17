# Redis Clustering
The following guide shows you how to configure shotover-proxy to support transparently proxying redis-cluster _unaware_ clients
to a [Redis cluster](https://redis.io/topics/cluster-spec).


## Overview
In this example, we will be connecting to a redis cluster that has the following topology:

* redis://192.168.0.1/
* redis://192.168.0.2/
* redis://192.168.0.3/
* redis://192.168.0.4/
* redis://192.168.0.5/
* redis://192.168.0.6/

Shotover will listen on the loopback adapter (localhost) and act as a sidecar for our application that speaks redis. In this example
we will use `redis-benchmark` as our redis-cluster unaware client application. 

## Configuration
First we will modify our config.yml file to have a single Redis source. This will: 

* define how shotover listens for incoming connections from our client application (`redis-benchmark`).
* configure shotover to connect to the redis cluster via our defined contact points
* connect our redis source to our redis cluster destination (transform).

The below configuration will do the trick.

```yaml
---
sources:
  redis_prod:
    Redis:
      listen_addr: "127.0.0.1:6379"
chain_config:
  redis_chain:
    - RedisCluster:
        first_contact_points: ["redis://192.168.0.1/", "redis://192.168.0.2/"]
named_topics:
  - testtopic
source_to_chain_mapping:
  redis_prod: redis_chain
```

Modify an existing `config.yml` or create a new one and place the above example as the files contents. Remember to change 
the `first_contact_points` to IPs and ports that matches your redis-cluster. In this example we will save our config as 
`redis-shotover.yml`.

## Starting
We can now start `shotover-proxy`, by running the following:

```
./shotover-proxy --config-file redis-shotover.yml
```

If shotover can successfully contact your redis cluster, you should see the following:

```
user@demo$ ./shotover-proxy --config-file redis-shotover.yml 
Aug 17 12:11:42.867  INFO instaproxy: Loading configuration
Aug 17 12:11:42.867  INFO instaproxy: Starting loaded topology
Aug 17 12:11:42.867  INFO instaproxy::config::topology: Loaded topics ["testtopic"]
Aug 17 12:11:42.876  INFO instaproxy::config::topology: Loaded chains ["redis_chain"]
Aug 17 12:11:42.876  INFO instaproxy::sources::redis_source: Starting Redis source on [127.0.0.1:6379]
Aug 17 12:11:42.878  INFO instaproxy::config::topology: Loaded sources [["redis_prod"]] and linked to chains
Aug 17 12:11:42.878  INFO instaproxy::server: accepting inbound connections
```

Currently the RedisCluster transform, needs to be able to connect to the redis cluster when it starts up. If it cannot, shotover
proxy will exit with a panic, indicating it couldn't connect to the contact points. 

Note: Currently `shotover-proxy` cannot daemonize itself. So you may wish to use a service supervisor to do this for you or you
can simply run this in a different terminal session in development/testing scenarios :)

## Testing
With shotover proxy now up and running, we can test out our client application. Let's start it up!
```
redis-benchmark -t set,get
```

And hooray we get the following:

```
====== SET ======
  100000 requests completed in 1.41 seconds
  50 parallel clients
  3 bytes payload
  keep alive: 1

88.81% <= 1 milliseconds
99.94% <= 2 milliseconds
99.95% <= 3 milliseconds
99.95% <= 5 milliseconds
99.96% <= 6 milliseconds
99.96% <= 7 milliseconds
99.96% <= 8 milliseconds
99.96% <= 9 milliseconds
99.96% <= 10 milliseconds
99.96% <= 11 milliseconds
99.96% <= 12 milliseconds
99.96% <= 13 milliseconds
99.96% <= 14 milliseconds
99.96% <= 16 milliseconds
99.96% <= 17 milliseconds
99.97% <= 18 milliseconds
99.97% <= 19 milliseconds
99.97% <= 20 milliseconds
99.97% <= 21 milliseconds
99.97% <= 22 milliseconds
99.97% <= 23 milliseconds
99.97% <= 25 milliseconds
99.97% <= 26 milliseconds
99.98% <= 27 milliseconds
99.98% <= 28 milliseconds
99.98% <= 29 milliseconds
99.98% <= 30 milliseconds
99.98% <= 31 milliseconds
99.98% <= 32 milliseconds
99.98% <= 33 milliseconds
99.98% <= 34 milliseconds
99.98% <= 35 milliseconds
99.99% <= 36 milliseconds
99.99% <= 37 milliseconds
99.99% <= 38 milliseconds
99.99% <= 40 milliseconds
99.99% <= 41 milliseconds
99.99% <= 42 milliseconds
99.99% <= 43 milliseconds
99.99% <= 45 milliseconds
100.00% <= 46 milliseconds
100.00% <= 47 milliseconds
100.00% <= 48 milliseconds
100.00% <= 49 milliseconds
71123.76 requests per second

====== GET ======
  100000 requests completed in 1.01 seconds
  50 parallel clients
  3 bytes payload
  keep alive: 1

98.90% <= 1 milliseconds
99.95% <= 2 milliseconds
99.95% <= 3 milliseconds
99.95% <= 4 milliseconds
99.96% <= 5 milliseconds
99.96% <= 6 milliseconds
99.96% <= 7 milliseconds
99.96% <= 8 milliseconds
99.96% <= 9 milliseconds
99.96% <= 10 milliseconds
99.96% <= 11 milliseconds
99.97% <= 12 milliseconds
99.97% <= 13 milliseconds
99.97% <= 14 milliseconds
99.97% <= 15 milliseconds
99.97% <= 16 milliseconds
99.97% <= 17 milliseconds
99.97% <= 18 milliseconds
99.97% <= 19 milliseconds
99.98% <= 20 milliseconds
99.98% <= 21 milliseconds
99.98% <= 22 milliseconds
99.98% <= 23 milliseconds
99.98% <= 25 milliseconds
99.98% <= 26 milliseconds
99.99% <= 27 milliseconds
99.99% <= 28 milliseconds
99.99% <= 29 milliseconds
99.99% <= 30 milliseconds
99.99% <= 31 milliseconds
99.99% <= 32 milliseconds
99.99% <= 33 milliseconds
99.99% <= 34 milliseconds
100.00% <= 35 milliseconds
100.00% <= 36 milliseconds
100.00% <= 37 milliseconds
100.00% <= 38 milliseconds
98522.17 requests per second
```