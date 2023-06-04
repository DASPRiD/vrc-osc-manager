# [1.1.0](https://github.com/DASPRiD/vrc-osc-manager/compare/v1.0.0...v1.1.0) (2023-06-04)


### Bug Fixes

* **tray:** perform clean shutdown on exit ([df5bbbc](https://github.com/DASPRiD/vrc-osc-manager/commit/df5bbbc3669fa79149b4b2c3dff02ca3e45e0a4f))


### Features

* **pishock:** add dynamic intensity cap ([700a70a](https://github.com/DASPRiD/vrc-osc-manager/commit/700a70a160c55059b241f72948c3fc8112ea362a))

# 1.0.0 (2023-05-21)


### Bug Fixes

* exclude variable from compilation when feature is not enabled ([933dac5](https://github.com/DASPRiD/vrc-osc-manager/commit/933dac56b0b0cd38a7a779716fd4b24a83c1871f))
* only set windows_subsystem to windows when not in debug mode ([de34a2e](https://github.com/DASPRiD/vrc-osc-manager/commit/de34a2eb68ddbbff7308dce7635218ebed62c7dc))
* **osc:** ignore error when sending fails due to no receivers ([afd1474](https://github.com/DASPRiD/vrc-osc-manager/commit/afd14742cc79ca4335910141a96f98c0cbfadc62))
* **pishock:** do not send quickshock when value is negative ([1b95b2f](https://github.com/DASPRiD/vrc-osc-manager/commit/1b95b2f9de50ded38204fb7e70d1cfb08eee8764))
* **pishock:** handle lagging receiver ([69736b6](https://github.com/DASPRiD/vrc-osc-manager/commit/69736b6eb53280363e6a9265c6be259d2fc8f51e))
* **tray:** use correct rc icon names ([c03ca5c](https://github.com/DASPRiD/vrc-osc-manager/commit/c03ca5cd13f25c1e042d4c11a3b361c5e6e14fec))


### Features

* add better logging facilities ([702f7aa](https://github.com/DASPRiD/vrc-osc-manager/commit/702f7aacc44755ad3bbe2ff7b6f3d0b74a5b9b39))
* add option to disable activity check ([95cd797](https://github.com/DASPRiD/vrc-osc-manager/commit/95cd797bce63ecbebb042b8568a6e8c876c08f7d))
* add rudimentary Windows support ([76d80bd](https://github.com/DASPRiD/vrc-osc-manager/commit/76d80bdeb5f6106a6ad23d5c68b0f68443d0ca09))
* add toggle to switch from light to dark mode icons ([08d098d](https://github.com/DASPRiD/vrc-osc-manager/commit/08d098dc0126335a2f964ad6bcac79c503034b8c))
* add tray icon support for windows ([7718c81](https://github.com/DASPRiD/vrc-osc-manager/commit/7718c81ec7134607431130bb7bdd0b0d5e4fb40c))
* add tray option to reload plugins ([37d3c97](https://github.com/DASPRiD/vrc-osc-manager/commit/37d3c973d45c1487c6b3b7704c534b0ce0172c66))
* allow picking specific plugins for compilation ([1ab8876](https://github.com/DASPRiD/vrc-osc-manager/commit/1ab88768ac7bd42e70ab7b76bc38ce77f29305c3))
* always run the the listener and sender in the background ([e0a02ad](https://github.com/DASPRiD/vrc-osc-manager/commit/e0a02ad26379fd3d98756656dc2b530662292416))
* initial commit ([9cea486](https://github.com/DASPRiD/vrc-osc-manager/commit/9cea486f6c749a0135afe8b3dac8514425320015))
* initial rework to plugin based architecture ([8a0b1fc](https://github.com/DASPRiD/vrc-osc-manager/commit/8a0b1fc99f79775176eda0f6ed247a3d90ab6fd2))
* **pishock:** add boolean parameter indicating shock activity ([874f1c2](https://github.com/DASPRiD/vrc-osc-manager/commit/874f1c2e7743558eb01bf3d82e6cd679a07b2ea2))
* **pishock:** add intensity cap ([8f9f32f](https://github.com/DASPRiD/vrc-osc-manager/commit/8f9f32fccef1ffaa4da7f6c2389934f16d141297))
* **pishock:** add quick shock ([aaa9e03](https://github.com/DASPRiD/vrc-osc-manager/commit/aaa9e030caa1193a8f73e16747e4ce5d2c7ee2cb))
* **pishock:** parse response and emit log output ([967eaf5](https://github.com/DASPRiD/vrc-osc-manager/commit/967eaf5cb9477e0e170a25071116354aed92ab53))
* **pishock:** send strength parameter when avatar loads ([0087c49](https://github.com/DASPRiD/vrc-osc-manager/commit/0087c4910ae6c5c23d92dc7f80b3e3e728cfc09b))
