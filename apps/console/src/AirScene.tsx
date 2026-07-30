import { useEffect, useRef } from 'react'
import * as THREE from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import type { AirPicture, Track } from './types'

type Props = {
  picture: AirPicture | null
  selectedId: string | null
  showTruth: boolean
  alertedZoneIds?: string[]
  onSelectTrack: (id: string) => void
}

export function AirScene({
  picture,
  selectedId,
  showTruth,
  alertedZoneIds = [],
  onSelectTrack,
}: Props) {
  const mountRef = useRef<HTMLDivElement>(null)
  const stateRef = useRef<{
    renderer: THREE.WebGLRenderer
    scene: THREE.Scene
    camera: THREE.PerspectiveCamera
    controls: OrbitControls
    trackGroup: THREE.Group
    truthGroup: THREE.Group
    sensorGroup: THREE.Group
    zoneGroup: THREE.Group
    effectorGroup: THREE.Group
    assetGroup: THREE.Group
    raycaster: THREE.Raycaster
    pointer: THREE.Vector2
    trackMeshes: Map<string, THREE.Mesh>
    truthMeshes: Map<string, THREE.Mesh>
  } | null>(null)

  useEffect(() => {
    const mount = mountRef.current
    if (!mount) return

    const scene = new THREE.Scene()
    scene.fog = new THREE.FogExp2(0x0b1210, 0.00018)

    const camera = new THREE.PerspectiveCamera(50, 1, 1, 20000)
    camera.position.set(-1800, 1600, 2200)
    camera.up.set(0, 0, 1)
    camera.lookAt(0, 0, 40)

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true })
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
    renderer.setClearColor(0x000000, 0)
    mount.appendChild(renderer.domElement)

    const controls = new OrbitControls(camera, renderer.domElement)
    controls.target.set(0, 0, 40)
    controls.enableDamping = true
    controls.dampingFactor = 0.07
    controls.enablePan = true
    controls.screenSpacePanning = false
    controls.minDistance = 250
    controls.maxDistance = 14000
    controls.maxPolarAngle = Math.PI * 0.49
    controls.minPolarAngle = 0.08
    controls.zoomSpeed = 1.1
    controls.rotateSpeed = 0.85
    controls.panSpeed = 0.8
    controls.update()

    const ambient = new THREE.AmbientLight(0xb7d0c0, 0.55)
    const key = new THREE.DirectionalLight(0xe8f0ea, 0.85)
    key.position.set(-1200, -800, 1800)
    scene.add(ambient, key)

    const ground = new THREE.Mesh(
      new THREE.CircleGeometry(5000, 64),
      new THREE.MeshStandardMaterial({
        color: 0x1a2a22,
        roughness: 0.92,
        metalness: 0.05,
      }),
    )
    scene.add(ground)

    const grid = new THREE.GridHelper(8000, 40, 0x3a5a48, 0x24362c)
    grid.rotation.x = Math.PI / 2
    ;(grid.material as THREE.Material).transparent = true
    ;(grid.material as THREE.Material).opacity = 0.35
    scene.add(grid)

    const zoneGroup = new THREE.Group()
    const sensorGroup = new THREE.Group()
    const trackGroup = new THREE.Group()
    const truthGroup = new THREE.Group()
    const effectorGroup = new THREE.Group()
    const assetGroup = new THREE.Group()
    scene.add(zoneGroup, sensorGroup, trackGroup, truthGroup, effectorGroup, assetGroup)

    const raycaster = new THREE.Raycaster()
    const pointer = new THREE.Vector2()
    const dragStart = { x: 0, y: 0 }
    let dragged = false

    const onResize = () => {
      const w = mount.clientWidth
      const h = mount.clientHeight
      camera.aspect = w / h
      camera.updateProjectionMatrix()
      renderer.setSize(w, h, false)
    }
    onResize()
    const ro = new ResizeObserver(onResize)
    ro.observe(mount)

    let frame = 0
    const animate = () => {
      frame = requestAnimationFrame(animate)
      controls.update()
      renderer.render(scene, camera)
    }
    animate()

    const onPointerDown = (ev: PointerEvent) => {
      dragStart.x = ev.clientX
      dragStart.y = ev.clientY
      dragged = false
    }
    const onPointerMove = (ev: PointerEvent) => {
      const dx = ev.clientX - dragStart.x
      const dy = ev.clientY - dragStart.y
      if (dx * dx + dy * dy > 36) dragged = true
    }
    const onPointerUp = (ev: PointerEvent) => {
      if (dragged || ev.button !== 0) return
      const rect = renderer.domElement.getBoundingClientRect()
      pointer.x = ((ev.clientX - rect.left) / rect.width) * 2 - 1
      pointer.y = -((ev.clientY - rect.top) / rect.height) * 2 + 1
      raycaster.setFromCamera(pointer, camera)
      const hits = raycaster.intersectObjects(trackGroup.children, false)
      if (hits[0]?.object.userData.trackId) {
        onSelectTrack(hits[0].object.userData.trackId as string)
      }
    }

    const el = renderer.domElement
    el.style.cursor = 'grab'
    el.addEventListener('pointerdown', onPointerDown)
    el.addEventListener('pointermove', onPointerMove)
    el.addEventListener('pointerup', onPointerUp)
    controls.addEventListener('start', () => {
      el.style.cursor = 'grabbing'
    })
    controls.addEventListener('end', () => {
      el.style.cursor = 'grab'
    })

    stateRef.current = {
      renderer,
      scene,
      camera,
      controls,
      trackGroup,
      truthGroup,
      sensorGroup,
      zoneGroup,
      effectorGroup,
      assetGroup,
      raycaster,
      pointer,
      trackMeshes: new Map(),
      truthMeshes: new Map(),
    }

    return () => {
      cancelAnimationFrame(frame)
      ro.disconnect()
      el.removeEventListener('pointerdown', onPointerDown)
      el.removeEventListener('pointermove', onPointerMove)
      el.removeEventListener('pointerup', onPointerUp)
      controls.dispose()
      renderer.dispose()
      mount.removeChild(renderer.domElement)
      stateRef.current = null
    }
  }, [onSelectTrack])

  useEffect(() => {
    const st = stateRef.current
    if (!st || !picture) return

    st.zoneGroup.clear()
    st.assetGroup.clear()
    for (const z of picture.zones) {
      const alerted = alertedZoneIds.includes(z.id)
      const color =
        z.kind === 'critical_asset'
          ? 0xe8a0a0
          : z.kind === 'keep_out' || z.kind === 'no_fly'
            ? 0xffb347
            : 0x4ecbff
      const ring = new THREE.Mesh(
        new THREE.RingGeometry(z.radius_m * 0.98, z.radius_m, 64),
        new THREE.MeshBasicMaterial({
          color,
          transparent: true,
          opacity: alerted ? 0.45 : 0.18,
          side: THREE.DoubleSide,
        }),
      )
      ring.position.set(z.center.x, z.center.y, 2)
      st.zoneGroup.add(ring)

      // Defended asset marker (not a track, not an effector).
      if (z.kind === 'critical_asset') {
        const asset = new THREE.Mesh(
          new THREE.CylinderGeometry(22, 28, 16, 6),
          new THREE.MeshStandardMaterial({
            color: 0xd4b896,
            roughness: 0.7,
            metalness: 0.1,
          }),
        )
        asset.rotation.x = Math.PI / 2
        asset.position.set(z.center.x, z.center.y, 12)
        st.assetGroup.add(asset)
      }
    }

    st.effectorGroup.clear()
    for (const ef of picture.effectors ?? []) {
      // Flat diamond = effector site (never a sphere/track).
      const mesh = new THREE.Mesh(
        new THREE.OctahedronGeometry(26, 0),
        new THREE.MeshStandardMaterial({
          color: ef.kind === 'jammer' ? 0xb8892e : 0xa33b3b,
          emissive: ef.active ? 0x553310 : 0x000000,
          emissiveIntensity: ef.active ? 0.65 : 0.05,
          flatShading: true,
        }),
      )
      mesh.position.set(ef.position.x, ef.position.y, ef.position.z + 18)
      st.effectorGroup.add(mesh)
    }

    st.sensorGroup.clear()
    for (const s of picture.sensors) {
      const mesh = new THREE.Mesh(
        new THREE.ConeGeometry(16, 36, 5),
        new THREE.MeshStandardMaterial({
          color: s.healthy ? 0x7ec8a3 : 0x555555,
          emissive: s.healthy ? 0x1a3a28 : 0x000000,
          emissiveIntensity: 0.35,
        }),
      )
      mesh.rotation.x = Math.PI / 2
      mesh.position.set(s.position.x, s.position.y, s.position.z)
      st.sensorGroup.add(mesh)

      const cov = new THREE.Mesh(
        new THREE.RingGeometry(s.range_m * 0.995, s.range_m, 90),
        new THREE.MeshBasicMaterial({
          color: 0x7ec8a3,
          transparent: true,
          opacity: s.healthy ? 0.04 : 0.012,
          side: THREE.DoubleSide,
        }),
      )
      cov.position.set(s.position.x, s.position.y, 1)
      st.sensorGroup.add(cov)
    }

    const seen = new Set<string>()
    for (const tr of picture.tracks) {
      seen.add(tr.id)
      let mesh = st.trackMeshes.get(tr.id)
      if (!mesh) {
        // Spheres = fused tracks only.
        mesh = new THREE.Mesh(
          new THREE.SphereGeometry(24, 12, 12),
          new THREE.MeshStandardMaterial({ color: 0xff5c5c }),
        )
        mesh.userData.trackId = tr.id
        st.trackGroup.add(mesh)
        st.trackMeshes.set(tr.id, mesh)
      }
      mesh.position.set(tr.position.x, tr.position.y, tr.position.z)
      const mat = mesh.material as THREE.MeshStandardMaterial
      mat.color.set(colorForTrack(tr, selectedId))
      if (tr.id === selectedId) {
        mat.emissive.set(0xc4f542)
        mat.emissiveIntensity = 0.55
      } else if (tr.zone_state === 'defended' || tr.zone_state === 'warning') {
        mat.emissive.set(0x3a1408)
        mat.emissiveIntensity = 0.28
      } else {
        mat.emissive.set(0x000000)
        mat.emissiveIntensity = 0
      }
      const scale = 0.8 + Math.min(tr.threat_score / 100, 1) * 0.8
      mesh.scale.setScalar(scale)
    }
    for (const [id, mesh] of st.trackMeshes) {
      if (!seen.has(id)) {
        st.trackGroup.remove(mesh)
        mesh.geometry.dispose()
        ;(mesh.material as THREE.Material).dispose()
        st.trackMeshes.delete(id)
      }
    }

    st.truthGroup.visible = showTruth
    const truthSeen = new Set<string>()
    if (showTruth) {
      for (const ent of picture.truth) {
        if (ent.role === 'friendly') continue
        truthSeen.add(ent.id)
        let mesh = st.truthMeshes.get(ent.id)
        if (!mesh) {
          // Magenta wireframe — debug-only; never solid like effectors.
          mesh = new THREE.Mesh(
            new THREE.OctahedronGeometry(28, 0),
            new THREE.MeshBasicMaterial({
              color: 0xe040a0,
              wireframe: true,
              transparent: true,
              opacity: 0.5,
              depthWrite: false,
            }),
          )
          st.truthGroup.add(mesh)
          st.truthMeshes.set(ent.id, mesh)
        }
        const mat = mesh.material as THREE.MeshBasicMaterial
        mat.color.setHex(
          ent.neutralized ? 0x554455 : ent.jammed ? 0xcc66aa : 0xe040a0,
        )
        mat.opacity = ent.neutralized ? 0.22 : 0.5
        mesh.position.set(ent.position.x, ent.position.y, ent.position.z)
      }
    }
    for (const [id, mesh] of st.truthMeshes) {
      if (!truthSeen.has(id)) {
        st.truthGroup.remove(mesh)
        mesh.geometry.dispose()
        ;(mesh.material as THREE.Material).dispose()
        st.truthMeshes.delete(id)
      }
    }
  }, [picture, selectedId, showTruth, alertedZoneIds])

  return <div className="viewport" ref={mountRef} />
}

function colorForTrack(tr: Track, selectedId: string | null) {
  if (tr.id === selectedId) return 0xc4f542
  if (tr.affiliation === 'friendly') return 0x4ecbff
  // RF-dark / fiber: cooler violet so not mistaken for RF hostiles.
  if (tr.rf_dark) {
    if (tr.zone_state === 'defended') return 0xc44dff
    if (tr.zone_state === 'warning') return 0x9b6dff
    return 0x7a5cff
  }
  if (tr.zone_state === 'defended') return 0xff2d2d
  if (tr.zone_state === 'warning') return 0xff6a3d
  if (tr.threat_score > 60) return 0xff5c5c
  if (tr.threat_score > 30) return 0xffb347
  return 0xd0d8d2
}
