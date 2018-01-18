/**
 * This module contains common useful definitions
 */

import * as THREE from "three";

export const Z_DOWN: THREE.Vector3 = new THREE.Vector3(0, 0, -1000000);
export const Z_UP: THREE.Vector3 = new THREE.Vector3(0, 0, 1000000);
export const ORIGIN: THREE.Vector3 = new THREE.Vector3(0, 0, 0);
export const POS_X: THREE.Vector3 = new THREE.Vector3(1, 0, 0);
export const NEG_X: THREE.Vector3 = new THREE.Vector3(-1, 0, 0);
export const POS_Y: THREE.Vector3 = new THREE.Vector3(0, 1, 0);
export const NEG_Y: THREE.Vector3 = new THREE.Vector3(0, -1, 0);
export const POS_Z: THREE.Vector3 = new THREE.Vector3(0, 0, 1);
export const NEG_Z: THREE.Vector3 = new THREE.Vector3(0, 0, -1);
